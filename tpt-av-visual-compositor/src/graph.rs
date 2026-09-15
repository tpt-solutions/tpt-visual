//! The compositing graph: nodes, edges, topological execution.

use crate::gpu::device::{CompositorError, Result};
use crate::gpu::pipeline::NodeParams;
use crate::node::{CompositorNode, NodeFrame, NodeId};
use std::sync::Arc;

/// A directed graph of compositing nodes.
///
/// `connect(producer, consumer, slot)` binds the producer's output to a
/// numbered input slot of the consumer. `execute` resolves a topological
/// order, wires input views, and renders every node; the last-added node
/// renders into the caller's final target.
pub struct CompositorGraph {
    nodes: Vec<Box<dyn CompositorNode>>,
    /// `slots[consumer][slot] = Some(producer)`.
    slots: Vec<Vec<Option<NodeId>>>,
}

impl Default for CompositorGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl CompositorGraph {
    /// An empty graph.
    #[must_use]
    pub fn new() -> Self {
        CompositorGraph {
            nodes: Vec::new(),
            slots: Vec::new(),
        }
    }

    /// Adds a node with `input_slots` input connections, returning its id.
    pub fn add_node(&mut self, node: Box<dyn CompositorNode>, input_slots: usize) -> NodeId {
        self.slots.push(vec![None; input_slots]);
        self.nodes.push(node);
        self.nodes.len() - 1
    }

    /// Connects `from`'s output into `to`'s input `slot`.
    ///
    /// # Errors
    /// Fails if ids or the slot are out of range.
    pub fn connect(&mut self, from: NodeId, to: NodeId, slot: usize) -> Result<()> {
        if from >= self.nodes.len() || to >= self.nodes.len() {
            return Err(CompositorError::InvalidOperation(format!(
                "connect {from} -> {to}: node count is {}",
                self.nodes.len()
            )));
        }
        let slot_ref = self.slots[to].get_mut(slot).ok_or_else(|| {
            CompositorError::InvalidOperation(format!("node {to} has no input slot {slot}"))
        })?;
        *slot_ref = Some(from);
        Ok(())
    }

    /// Number of nodes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the graph has no nodes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Topological node order (Kahn's algorithm). Errors on cycles or
    /// unconnected inputs.
    fn topological_order(&self) -> Result<Vec<NodeId>> {
        let n = self.nodes.len();
        let mut in_degree = vec![0_usize; n];
        let mut consumers: Vec<Vec<NodeId>> = vec![Vec::new(); n];
        for (consumer, slots) in self.slots.iter().enumerate() {
            for slot in slots {
                match slot {
                    Some(producer) => {
                        in_degree[consumer] += 1;
                        consumers[*producer].push(consumer);
                    }
                    None => {
                        return Err(CompositorError::InvalidOperation(format!(
                            "node {consumer} has an unconnected input"
                        )));
                    }
                }
            }
        }
        let mut queue: Vec<NodeId> = (0..n).filter(|&i| in_degree[i] == 0).collect();
        let mut order = Vec::with_capacity(n);
        while let Some(node) = queue.pop() {
            order.push(node);
            for &consumer in &consumers[node] {
                in_degree[consumer] -= 1;
                if in_degree[consumer] == 0 {
                    queue.push(consumer);
                }
            }
        }
        if order.len() != n {
            return Err(CompositorError::InvalidOperation(
                "compositing graph contains a cycle".into(),
            ));
        }
        Ok(order)
    }

    /// Executes the graph for `frame`, rendering the last-added node into
    /// `final_target`.
    ///
    /// The execution order is topological; every node receives its inputs'
    /// output views before rendering. Intermediate render targets come from
    /// the shared texture pool and are recycled afterwards.
    pub fn execute(
        &mut self,
        ctx: &mut NodeFrame,
        final_target: &wgpu::TextureView,
        params: &NodeParams,
        frame: u64,
    ) -> Result<()> {
        let order = self.topological_order()?;
        if order.is_empty() {
            return Err(CompositorError::InvalidOperation(
                "compositing graph is empty".into(),
            ));
        }
        let n = self.nodes.len();
        let mut views: Vec<Option<Arc<wgpu::TextureView>>> = (0..n).map(|_| None).collect();
        let mut pooled: Vec<Option<wgpu::Texture>> = (0..n).map(|_| None).collect();
        let final_node = order[order.len() - 1];

        for &node in &order {
            // Wire inputs.
            let slots = self.slots[node].clone();
            for (slot, producer) in slots.iter().enumerate() {
                let Some(producer) = producer else { continue };
                if let Some(view) = views[*producer].clone() {
                    self.nodes[node].set_input(slot, view);
                }
            }

            if node == final_node {
                self.nodes[node].render(ctx, final_target, params, frame)?;
                continue;
            }
            if let Some(passthrough) = self.nodes[node].output_override() {
                views[node] = Some(passthrough);
                continue;
            }
            let texture = ctx.pool.acquire(
                ctx.device,
                ctx.resolution.width,
                ctx.resolution.height,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
            );
            let view = Arc::new(texture.create_view(&wgpu::TextureViewDescriptor::default()));
            self.nodes[node].render(ctx, &view, params, frame)?;
            views[node] = Some(view);
            pooled[node] = Some(texture);
        }

        // Recycle every intermediate texture; the final target belongs to
        // the caller and passthrough views are not pooled.
        for texture in pooled.into_iter().flatten() {
            ctx.pool.release(texture);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycle_detection() {
        struct Fake;
        impl CompositorNode for Fake {
            fn render(
                &mut self,
                _: &mut NodeFrame,
                _: &wgpu::TextureView,
                _: &NodeParams,
                _: u64,
            ) -> Result<()> {
                Ok(())
            }
            fn inputs(&self) -> Vec<NodeId> {
                vec![]
            }
            fn set_input(&mut self, _: usize, _: Arc<wgpu::TextureView>) {}
        }
        let mut graph = CompositorGraph::new();
        graph.add_node(Box::new(Fake), 1);
        graph.add_node(Box::new(Fake), 1);
        graph.connect(1, 0, 0).unwrap();
        graph.connect(0, 1, 0).unwrap();
        assert!(graph.topological_order().is_err());
    }

    #[test]
    fn unconnected_inputs_rejected() {
        struct Fake;
        impl CompositorNode for Fake {
            fn render(
                &mut self,
                _: &mut NodeFrame,
                _: &wgpu::TextureView,
                _: &NodeParams,
                _: u64,
            ) -> Result<()> {
                Ok(())
            }
            fn inputs(&self) -> Vec<NodeId> {
                vec![]
            }
            fn set_input(&mut self, _: usize, _: Arc<wgpu::TextureView>) {}
        }
        let mut graph = CompositorGraph::new();
        graph.add_node(Box::new(Fake), 0);
        graph.add_node(Box::new(Fake), 2);
        graph.connect(0, 1, 0).unwrap();
        assert!(graph.topological_order().is_err());
    }

    #[test]
    fn connect_validates_ranges() {
        struct Fake;
        impl CompositorNode for Fake {
            fn render(
                &mut self,
                _: &mut NodeFrame,
                _: &wgpu::TextureView,
                _: &NodeParams,
                _: u64,
            ) -> Result<()> {
                Ok(())
            }
            fn inputs(&self) -> Vec<NodeId> {
                vec![]
            }
            fn set_input(&mut self, _: usize, _: Arc<wgpu::TextureView>) {}
        }
        let mut graph = CompositorGraph::new();
        graph.add_node(Box::new(Fake), 0);
        assert!(graph.connect(5, 0, 0).is_err());
        assert!(graph.connect(0, 0, 3).is_err());
    }
}
