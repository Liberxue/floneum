use crate::raw::cache::LlamaCache;
use crate::{raw::Model, session::LlamaSession};
use anyhow::{Error as E, Result};
use kalosm_common::*;
use kalosm_language_model::SyncModelExt;
use std::sync::Arc;

use candle_core::{
    quantized::{ggml_file, gguf_file},
    DType, Device,
};
use kalosm_language_model::SyncModel;
use tokenizers::Tokenizer;

use crate::InferenceSettings;

/// The inner, synchronous Llama model.
pub struct LlamaModel {
    model: Model,
    device: Device,
    tokenizer: Arc<Tokenizer>,
    cache: LlamaCache,
}

impl SyncModel for LlamaModel {
    type Session = LlamaSession;

    fn new_session(&self) -> anyhow::Result<Self::Session> {
        let cache = self.cache.clone();
        Ok(Self::Session { cache })
    }

    fn feed_text(
        &self,
        session: &mut Self::Session,
        prompt: &str,
        logits: &mut Vec<f32>,
    ) -> anyhow::Result<()> {
        let encoded = self.tokenizer.encode(prompt, false).map_err(E::msg)?;
        let tokens = encoded.get_ids();
        self.feed_tokens(session, tokens, logits)
    }

    fn feed_tokens(
        &self,
        session: &mut Self::Session,
        tokens: &[u32],
        logits: &mut Vec<f32>,
    ) -> anyhow::Result<()> {
        Self::forward(
            &self.model,
            &self.device,
            tokens,
            Some(&mut session.cache),
            logits,
        )
    }

    fn stop_token(&self) -> anyhow::Result<u32> {
        let vocab = self.tokenizer.get_vocab(true);
        let eos_token = match vocab.get("</s>").or(vocab.get("<|end_of_text|>")) {
            Some(token) => *token,
            None => anyhow::bail!("cannot find the </s> token"),
        };
        Ok(eos_token)
    }

    fn tokenizer(&self) -> Arc<Tokenizer> {
        self.tokenizer.clone()
    }
}

impl LlamaModel {
    fn forward(
        model: &Model,
        device: &Device,
        tokens: &[u32],
        cache: Option<&mut LlamaCache>,
        logits_vec: &mut Vec<f32>,
    ) -> anyhow::Result<()> {
        if tokens.is_empty() {
            return Err(anyhow::anyhow!("Cannot run model on empty input"));
        }

        let logits = model.forward(tokens, device, cache)?;

        let logits = logits.squeeze(0)?.to_dtype(DType::F32)?;
        copy_tensor_into_vec(&logits, logits_vec)?;

        Ok(())
    }

    /// Create a new sync Llama model from a builder.
    pub async fn from_builder(
        builder: crate::LlamaBuilder,
        mut handler: impl FnMut(ModelLoadingProgress) + Send + Sync + 'static,
    ) -> anyhow::Result<Self> {
        let device = builder.get_device()?;

        let tokenizer_source = format!("Tokenizer ({})", builder.source.tokenizer);
        let mut create_progress = ModelLoadingProgress::downloading_progress(tokenizer_source);
        let tokenizer = builder
            .source
            .tokenizer(|progress| handler(create_progress(progress)))
            .await?;

        let source = format!("Model ({})", builder.source.model);
        let mut create_progress = ModelLoadingProgress::downloading_progress(source);
        let filename = builder
            .source
            .model(|progress| handler(create_progress(progress)))
            .await?;
        let mut file = std::fs::File::open(&filename)?;
        let model = match filename.extension().and_then(|v| v.to_str()) {
            Some("gguf") => {
                let model = gguf_file::Content::read(&mut file)?;
                Model::from_gguf(model, &mut file, &device)?
            }
            Some("ggml" | "bin") | Some(_) | None => {
                let model = ggml_file::Content::read(&mut file, &device)?;
                let gqa = builder.source.group_query_attention;
                Model::from_ggml(model, gqa as usize, &device)?
            }
        };

        let cache = LlamaCache::new(&model.config);
        Ok(Self {
            model,
            tokenizer: Arc::new(tokenizer),
            device,
            cache,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        model: Model,
        tokenizer: Arc<Tokenizer>,
        device: Device,
        cache: LlamaCache,
    ) -> Self {
        Self {
            cache,
            model,
            device,
            tokenizer,
        }
    }

    pub(crate) fn _infer(
        &mut self,
        settings: InferenceSettings,
        sampler: std::sync::Arc<std::sync::Mutex<dyn llm_samplers::prelude::Sampler>>,
        out: tokio::sync::mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        let InferenceSettings {
            prompt,
            sample_len,
            stop_on,
        } = settings;

        let mut session = self.new_session()?;

        self.stream_text_with_sampler(
            &mut session,
            prompt.as_str(),
            Some(sample_len as u32),
            stop_on.as_deref(),
            sampler,
            |token| {
                out.send(token)
                    .map_err(|_| anyhow::anyhow!("Failed to send token to output channel"))
                    .map(|_| kalosm_language_model::ModelFeedback::Continue)
            },
        )?;

        Ok(())
    }
}
