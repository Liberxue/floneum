use rss::Channel;
use url::Url;

use crate::index::IntoDocuments;

use super::document::Document;

#[derive(Debug, Clone, PartialEq)]
pub struct RssFeed(Url);

impl From<Url> for RssFeed {
    fn from(url: Url) -> Self {
        Self::new(url)
    }
}

#[async_trait::async_trait]
impl IntoDocuments for RssFeed {
    async fn into_documents(self) -> anyhow::Result<Vec<Document>> {
        self.read_top_n(usize::MAX).await
    }
}

impl RssFeed {
    pub fn new(url: Url) -> Self {
        Self(url)
    }

    pub fn url(&self) -> &Url {
        &self.0
    }

    pub async fn read_top_n(&self, top_n: usize) -> anyhow::Result<Vec<Document>> {
        let xml = reqwest::get(self.0.clone()).await?.text().await?;
        let channel = Channel::read_from(xml.as_bytes())?;
        let mut documents = Vec::new();
        for item in channel.items().iter().take(top_n) {
            let mut message = String::new();
            if let Some(title) = item.title() {
                message.push_str(&format!("### {}\n", title));
            }
            let (source_url, content) = if let Some(content) = item.content() {
                (None, content.to_string())
            } else if let Some(source_url) = item.link() {
                (
                    Some(source_url),
                    reqwest::get(source_url).await?.text().await?,
                )
            } else {
                (None, String::new())
            };

            let url = match source_url {
                Some(url) => Url::parse(url).unwrap(),
                None => self.0.clone(),
            };

            let article =
                readability::extractor::extract(&mut std::io::Cursor::new(&content), &url)?;

            documents.push(Document::from_parts(article.title, article.text));
        }
        Ok(documents)
    }
}
