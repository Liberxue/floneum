use crate::host::{AnyNodeRef, State};
use crate::resource::Resource;
use crate::resource::ResourceStorage;

use crate::plugins::main::types::Node;

impl State {
    pub fn with_node<O>(
        &self,
        node: Node,
        f: impl FnOnce(headless_chrome::Element) -> anyhow::Result<O>,
    ) -> anyhow::Result<O> {
        let index = node.into();
        let node = self.resources.get(index).ok_or(anyhow::anyhow!(
            "Node not found; may have been already dropped"
        ))?;
        let tab = Resource::from_index_borrowed(node.page_id);
        let tab = self.resources.get(tab).ok_or(anyhow::anyhow!(
            "Page not found; may have been already dropped"
        ))?;
        f(headless_chrome::Element::new(&tab, node.node_id)?)
    }
}

impl State {
    pub(crate) async fn impl_get_element_text(&mut self, self_: Node) -> wasmtime::Result<String> {
        self.with_node(self_, |node| node.get_inner_text())
    }

    pub(crate) async fn impl_click_element(&mut self, self_: Node) -> wasmtime::Result<()> {
        self.with_node(self_, |node| {
            node.click()?;
            Ok(())
        })?;
        Ok(())
    }

    pub(crate) async fn impl_type_into_element(
        &mut self,
        self_: Node,
        keys: String,
    ) -> wasmtime::Result<()> {
        self.with_node(self_, |node| {
            node.type_into(&keys)?;
            Ok(())
        })
    }

    pub(crate) async fn impl_get_element_outer_html(
        &mut self,
        self_: Node,
    ) -> wasmtime::Result<String> {
        self.with_node(self_, |node| node.get_content())
    }

    pub(crate) async fn impl_screenshot_element(
        &mut self,
        self_: Node,
    ) -> wasmtime::Result<Vec<u8>> {
        self.with_node(self_, |node| {
            node.capture_screenshot(
                headless_chrome::protocol::cdp::Page::CaptureScreenshotFormatOption::Jpeg,
            )
        })
    }

    pub(crate) async fn impl_find_child_of_element(
        &mut self,
        self_: Node,
        query: String,
    ) -> wasmtime::Result<Node> {
        let node = {
            let index = self_.into();
            let node = self
                .resources
                .get(index)
                .ok_or(anyhow::anyhow!("Node not found"))?;
            node
        };
        let child = {
            let page = Resource::from_index_borrowed(page_id);
            let tab = self
                .resources
                .get(page)
                .ok_or(anyhow::anyhow!("Page not found"))?;
            let node = headless_chrome::Element::new(&*tab, node.node_id)?;
            node.find_element(&query)?
        };
        let child = self.resources.insert(AnyNodeRef {
            page_id: node.page_id,
            node_id: child.node_id,
        });
        Ok(Node {
            id: child.index() as u64,
            owned: true,
        })
    }

    pub(crate) fn impl_drop_node(&mut self, rep: Node) -> wasmtime::Result<()> {
        let index = rep.into();
        self.resources.drop_key(index);
        Ok(())
    }
}
