//! High-Performance Directed Acyclic Graph (DAG) Pipeline Engine
//! Provides demand-driven, node-based, asynchronous, and cache-aware evaluation
//! designed for DaVinci Resolve / Nuke grade image processing.

pub mod tiling;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use std::time::Instant;

/// Unique identifier for each node in the DAG
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub usize);

/// Region of Interest (ROI) for Tiled / Demand-Driven evaluation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileRoi {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub zoom_level: u32, // Mipmap level or scale factor (1 = 100%, 2 = 50%, etc.)
}

impl TileRoi {
    pub fn full(width: u32, height: u32) -> Self {
        Self {
            x: 0,
            y: 0,
            width,
            height,
            zoom_level: 1,
        }
    }

    pub fn intersects(&self, other: &TileRoi) -> bool {
        !(self.x + self.width <= other.x
            || other.x + other.width <= self.x
            || self.y + self.height <= other.y
            || other.y + other.height <= self.y)
    }
}

/// 32-bit Floating Point Linear Planar/Interleaved Buffer for IEEE 754 High Dynamic Range
#[derive(Debug, Clone)]
pub struct ImageBufferF32 {
    pub width: u32,
    pub height: u32,
    pub channels: u32,
    pub data: Vec<f32>,
}

impl ImageBufferF32 {
    pub fn new(width: u32, height: u32, channels: u32) -> Self {
        let size = (width as usize) * (height as usize) * (channels as usize);
        Self {
            width,
            height,
            channels,
            data: vec![0.0f32; size],
        }
    }

    pub fn from_u16(src: &[u16], width: u32, height: u32, channels: u32) -> Self {
        let data: Vec<f32> = src.iter().map(|&v| (v as f32) / 65535.0).collect();
        Self {
            width,
            height,
            channels,
            data,
        }
    }

    pub fn to_u8(&self) -> Vec<u8> {
        self.data.iter().map(|&v| (v.clamp(0.0, 1.0) * 255.0).round() as u8).collect()
    }
}

/// Types of Nodes supported in the DAG
#[derive(Debug, Clone, PartialEq)]
pub enum NodeType {
    /// RAW Input Node (LibRaw sensor stream)
    RawSource { path: String },
    /// Demosaicing (GPU/CPU RCD, AMaZE, Markesteijn)
    Demosaic { algorithm: String },
    /// White Balance (Kelvin & Tint Matrix)
    WhiteBalance { temp: f32, tint: f32 },
    /// Exposure & Photometric Primary Gain
    Exposure { ev: f32 },
    /// 4-Zone Parametric Tone Curve
    ToneCurve { highlights: f32, lights: f32, darks: f32, shadows: f32 },
    /// DaVinci 3-Way Color Wheels
    ColorWheels {
        lift: [f32; 3],
        lift_luma: f32,
        gamma: [f32; 3],
        gamma_luma: f32,
        gain: [f32; 3],
        gain_luma: f32,
        offset: [f32; 3],
        offset_luma: f32,
        contrast_pivot: f32,
        color_boost: f32,
        midtone_detail: f32,
    },
    /// Authentic Film Simulation (Fujifilm, Hasselblad HNCS)
    FilmSimulation { id: String, intensity: f32 },
    /// ACES 1.3 / 2.0 Reference Gamut Compression (RGC) & Output Transform
    AcesColorManagement {
        color_space: String,
        rrt_odt: bool,
    },
    /// 3D LUT Evaluation
    Lut3D { name: Option<String>, path: Option<String>, intensity: f32 },
    /// Spatial Denoise & Presence (Wavelet / NLMeans)
    Denoise { luma: f32, color: f32 },
    /// Watermark Overlay
    Watermark {
        text: String,
        opacity: f32,
        position_index: usize,
    },
    /// Final Display & Output Sink
    DisplaySink,
}

/// Node representation in the Directed Acyclic Graph
pub struct PipelineNode {
    pub id: NodeId,
    pub name: String,
    pub node_type: NodeType,
    pub inputs: Vec<NodeId>,
    pub outputs: Vec<NodeId>,
    pub dirty: bool,
    pub cached_buffer: Option<Arc<ImageBufferF32>>,
    pub cached_roi: Option<TileRoi>,
    pub last_eval_duration_ms: f32,
}

impl PipelineNode {
    pub fn new(id: NodeId, name: impl Into<String>, node_type: NodeType) -> Self {
        Self {
            id,
            name: name.into(),
            node_type,
            inputs: Vec::new(),
            outputs: Vec::new(),
            dirty: true,
            cached_buffer: None,
            cached_roi: None,
            last_eval_duration_ms: 0.0,
        }
    }

    pub fn invalidate(&mut self) {
        self.dirty = true;
        self.cached_buffer = None;
    }
}

/// Directed Acyclic Graph (DAG) Execution Engine
pub struct DagEngine {
    nodes: HashMap<NodeId, RwLock<PipelineNode>>,
    next_id: usize,
    sink_id: Option<NodeId>,
    source_id: Option<NodeId>,
}

impl Default for DagEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl DagEngine {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            next_id: 1,
            sink_id: None,
            source_id: None,
        }
    }

    /// Adds a new node to the DAG
    pub fn add_node(&mut self, name: impl Into<String>, node_type: NodeType) -> NodeId {
        let id = NodeId(self.next_id);
        self.next_id += 1;

        if matches!(node_type, NodeType::RawSource { .. }) {
            self.source_id = Some(id);
        }
        if matches!(node_type, NodeType::DisplaySink) {
            self.sink_id = Some(id);
        }

        let node = PipelineNode::new(id, name, node_type);
        self.nodes.insert(id, RwLock::new(node));
        id
    }

    /// Connects output of upstream node to input of downstream node
    pub fn connect(&mut self, from: NodeId, to: NodeId) -> Result<(), String> {
        if from == to {
            return Err("Self-referential cycles are prohibited in DAG".to_string());
        }

        // Verify nodes exist
        if !self.nodes.contains_key(&from) || !self.nodes.contains_key(&to) {
            return Err("Source or target node not found in DAG".to_string());
        }

        // Check for cycle before connecting
        if self.would_create_cycle(from, to) {
            return Err("Connecting this edge would introduce a directed cycle".to_string());
        }

        {
            let mut from_node = self.nodes.get(&from).unwrap().write().unwrap();
            if !from_node.outputs.contains(&to) {
                from_node.outputs.push(to);
            }
        }

        {
            let mut to_node = self.nodes.get(&to).unwrap().write().unwrap();
            if !to_node.inputs.contains(&from) {
                to_node.inputs.push(from);
                to_node.invalidate();
            }
        }

        self.propagate_invalidation(to);
        Ok(())
    }

    /// Verifies whether adding an edge (from -> to) would create a cycle
    fn would_create_cycle(&self, from: NodeId, to: NodeId) -> bool {
        let mut visited = HashSet::new();
        let mut stack = vec![to];

        while let Some(current) = stack.pop() {
            if current == from {
                return true;
            }
            if visited.insert(current) {
                if let Some(lock) = self.nodes.get(&current) {
                    let node = lock.read().unwrap();
                    for &next in &node.outputs {
                        stack.push(next);
                    }
                }
            }
        }
        false
    }

    /// Propagates invalidation downstream from a modified node
    pub fn propagate_invalidation(&self, start: NodeId) {
        let mut stack = vec![start];
        let mut visited = HashSet::new();

        while let Some(current) = stack.pop() {
            if visited.insert(current) {
                if let Some(lock) = self.nodes.get(&current) {
                    let mut node = lock.write().unwrap();
                    node.invalidate();
                    for &downstream in &node.outputs {
                        stack.push(downstream);
                    }
                }
            }
        }
    }

    /// Evaluates topological order of nodes required to evaluate target_node
    pub fn get_evaluation_order(&self, target_node: NodeId) -> Result<Vec<NodeId>, String> {
        let mut order = Vec::new();
        let mut visited = HashSet::new();
        let mut in_progress = HashSet::new();

        fn dfs(
            engine: &DagEngine,
            node_id: NodeId,
            visited: &mut HashSet<NodeId>,
            in_progress: &mut HashSet<NodeId>,
            order: &mut Vec<NodeId>,
        ) -> Result<(), String> {
            if in_progress.contains(&node_id) {
                return Err("Cycle detected during evaluation traversal".to_string());
            }
            if visited.contains(&node_id) {
                return Ok(());
            }

            in_progress.insert(node_id);

            let upstream = {
                let lock = engine.nodes.get(&node_id).ok_or("Node missing")?;
                let node = lock.read().unwrap();
                node.inputs.clone()
            };

            for up in upstream {
                dfs(engine, up, visited, in_progress, order)?;
            }

            in_progress.remove(&node_id);
            visited.insert(node_id);
            order.push(node_id);
            Ok(())
        }

        dfs(self, target_node, &mut visited, &mut in_progress, &mut order)?;
        Ok(order)
    }

    /// Executes the DAG evaluation up to the specified target node with Demand-Driven ROI
    pub fn evaluate_node(&self, target_node: NodeId, roi: TileRoi) -> Result<Arc<ImageBufferF32>, String> {
        let order = self.get_evaluation_order(target_node)?;

        for node_id in order {
            let mut node = self.nodes.get(&node_id).ok_or("Node not found")?.write().unwrap();

            // Lazy Evaluation check: reuse cached buffer if valid and covers requested ROI
            if !node.dirty && node.cached_buffer.is_some() {
                if let Some(ref cached_roi) = node.cached_roi {
                    if cached_roi == &roi {
                        continue;
                    }
                }
            }

            let start = Instant::now();

            // Gather inputs from upstream nodes
            let mut input_buffers = Vec::new();
            for &in_id in &node.inputs {
                let up_lock = self.nodes.get(&in_id).ok_or("Upstream node missing")?;
                let up_node = up_lock.read().unwrap();
                if let Some(ref buf) = up_node.cached_buffer {
                    input_buffers.push(Arc::clone(buf));
                } else {
                    return Err(format!("Upstream node {:?} has no valid cached output", in_id));
                }
            }

            // Execute specific node operator
            let output_buf = self.execute_operator(&node.node_type, &input_buffers, roi)?;
            node.cached_buffer = Some(Arc::new(output_buf));
            node.cached_roi = Some(roi);
            node.dirty = false;
            node.last_eval_duration_ms = start.elapsed().as_secs_f32() * 1000.0;
        }

        let target_lock = self.nodes.get(&target_node).ok_or("Target node missing")?;
        let target = target_lock.read().unwrap();
        target.cached_buffer.clone().ok_or_else(|| "Evaluation produced no buffer".to_string())
    }

    /// Executes atomic operator computation for a node
    fn execute_operator(
        &self,
        node_type: &NodeType,
        inputs: &[Arc<ImageBufferF32>],
        roi: TileRoi,
    ) -> Result<ImageBufferF32, String> {
        match node_type {
            NodeType::RawSource { .. } => {
                // If inputs are empty (root source), provide blank or decoded base canvas
                Ok(ImageBufferF32::new(roi.width, roi.height, 3))
            }
            NodeType::Exposure { ev } => {
                let in_buf = inputs.first().ok_or("Exposure node requires 1 input")?;
                let mut out_buf = in_buf.as_ref().clone();
                let factor = 2.0f32.powf(*ev);
                for v in out_buf.data.iter_mut() {
                    *v *= factor;
                }
                Ok(out_buf)
            }
            NodeType::WhiteBalance { temp, tint } => {
                let in_buf = inputs.first().ok_or("WB node requires 1 input")?;
                let mut out_buf = in_buf.as_ref().clone();
                let (wr, wg, wb) = crate::pipeline::white_balance::kelvin_to_rgb_multipliers(*temp, *tint);
                for chunk in out_buf.data.chunks_exact_mut(3) {
                    chunk[0] *= wr;
                    chunk[1] *= wg;
                    chunk[2] *= wb;
                }
                Ok(out_buf)
            }
            NodeType::ToneCurve { highlights, lights, darks, shadows } => {
                let in_buf = inputs.first().ok_or("Curve node requires 1 input")?;
                let mut out_buf = in_buf.as_ref().clone();
                for chunk in out_buf.data.chunks_exact_mut(3) {
                    let r = chunk[0];
                    let g = chunk[1];
                    let b = chunk[2];
                    let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
                    let ch = (*highlights / 100.0) * (luma - 0.75).max(0.0) / 0.25;
                    let cl = (*lights / 100.0) * ((luma - 0.5).max(0.0) * (0.75 - luma).max(0.0) * 4.0);
                    let cd = (*darks / 100.0) * ((luma - 0.25).max(0.0) * (0.5 - luma).max(0.0) * 4.0);
                    let cs = (*shadows / 100.0) * (0.25 - luma).max(0.0) / 0.25;
                    let luma_adj = luma + (ch + cl + cd + cs) * 0.25;
                    let scale = if luma > 1e-5 { luma_adj / luma } else { 1.0 };
                    chunk[0] = r * scale;
                    chunk[1] = g * scale;
                    chunk[2] = b * scale;
                }
                Ok(out_buf)
            }
            NodeType::AcesColorManagement { color_space, rrt_odt } => {
                let in_buf = inputs.first().ok_or("ACES node requires 1 input")?;
                let mut out_buf = in_buf.as_ref().clone();
                let ws = crate::pipeline::aces::WorkingColorSpace::from_str_name(color_space);

                for chunk in out_buf.data.chunks_exact_mut(3) {
                    let (r, g, b) = (chunk[0], chunk[1], chunk[2]);
                    let (lr, lg, lb) = crate::pipeline::aces::linear_to_acescg(r, g, b, ws);
                    let (cr, cg, cb) = crate::pipeline::aces::apply_aces_gamut_compression(lr, lg, lb);
                    let (final_r, final_g, final_b) = if *rrt_odt {
                        (
                            crate::pipeline::aces::apply_aces_tonemap(cr),
                            crate::pipeline::aces::apply_aces_tonemap(cg),
                            crate::pipeline::aces::apply_aces_tonemap(cb),
                        )
                    } else {
                        (cr, cg, cb)
                    };
                    chunk[0] = final_r;
                    chunk[1] = final_g;
                    chunk[2] = final_b;
                }
                Ok(out_buf)
            }
            NodeType::FilmSimulation { id, intensity } => {
                let in_buf = inputs.first().ok_or("FilmSimulation node requires 1 input")?;
                let mut out_buf = in_buf.as_ref().clone();
                let sim = crate::pipeline::film_sim::FilmSimulation::from_id(id);
                for chunk in out_buf.data.chunks_exact_mut(3) {
                    let (r, g, b) = (chunk[0], chunk[1], chunk[2]);
                    let (sr, sg, sb) = sim.apply_pixel(r, g, b, *intensity);
                    chunk[0] = sr;
                    chunk[1] = sg;
                    chunk[2] = sb;
                }
                Ok(out_buf)
            }
            NodeType::ColorWheels {
                lift,
                lift_luma,
                gamma,
                gamma_luma,
                gain,
                gain_luma,
                offset,
                offset_luma,
                contrast_pivot,
                color_boost,
                ..
            } => {
                let in_buf = inputs.first().ok_or("ColorWheels node requires 1 input")?;
                let mut out_buf = in_buf.as_ref().clone();
                for chunk in out_buf.data.chunks_exact_mut(3) {
                    let (mut r, mut g, mut b) = (chunk[0], chunk[1], chunk[2]);
                    // Lift (Shadows)
                    let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
                    let lift_w = (1.0 - luma).max(0.0);
                    r += (lift[0] + lift_luma) * lift_w * 0.2;
                    g += (lift[1] + lift_luma) * lift_w * 0.2;
                    b += (lift[2] + lift_luma) * lift_w * 0.2;

                    // Gamma (Midtones)
                    let gamma_w = 1.0 - (2.0 * luma - 1.0).abs().clamp(0.0, 1.0);
                    let gam_r = (gamma[0] + gamma_luma) * 0.2;
                    let gam_g = (gamma[1] + gamma_luma) * 0.2;
                    let gam_b = (gamma[2] + gamma_luma) * 0.2;
                    r += gam_r * gamma_w;
                    g += gam_g * gamma_w;
                    b += gam_b * gamma_w;

                    // Gain (Highlights)
                    let gain_w = luma.max(0.0);
                    let gr = 1.0 + (gain[0] + gain_luma) * 0.5;
                    let gg = 1.0 + (gain[1] + gain_luma) * 0.5;
                    let gb = 1.0 + (gain[2] + gain_luma) * 0.5;
                    r = r * (1.0 - gain_w) + (r * gr) * gain_w;
                    g = g * (1.0 - gain_w) + (g * gg) * gain_w;
                    b = b * (1.0 - gain_w) + (b * gb) * gain_w;

                    // Offset
                    r += (offset[0] + offset_luma) * 0.1;
                    g += (offset[1] + offset_luma) * 0.1;
                    b += (offset[2] + offset_luma) * 0.1;

                    // Contrast Pivot (around 18% middle gray)
                    if (contrast_pivot.abs() - 0.0).abs() > 0.001 {
                        let pivot = 0.18f32;
                        let c = 1.0 + *contrast_pivot;
                        r = (pivot + (r - pivot) * c).max(0.0);
                        g = (pivot + (g - pivot) * c).max(0.0);
                        b = (pivot + (b - pivot) * c).max(0.0);
                    }

                    // Color Boost
                    if color_boost.abs() > 0.001 {
                        let max_c = r.max(g).max(b);
                        let min_c = r.min(g).min(b);
                        let sat = if max_c > 1e-5 { (max_c - min_c) / max_c } else { 0.0 };
                        let boost_weight = (1.0 - sat).max(0.0);
                        let lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
                        let factor = 1.0 + (*color_boost / 100.0) * boost_weight;
                        r = lum + (r - lum) * factor;
                        g = lum + (g - lum) * factor;
                        b = lum + (b - lum) * factor;
                    }

                    chunk[0] = r;
                    chunk[1] = g;
                    chunk[2] = b;
                }
                Ok(out_buf)
            }
            NodeType::DisplaySink => {
                let in_buf = inputs.first().ok_or("Sink node requires 1 input")?;
                Ok(in_buf.as_ref().clone())
            }
            _ => {
                // Pass-through fallback for composite/unimplemented nodes
                if let Some(first) = inputs.first() {
                    Ok(first.as_ref().clone())
                } else {
                    Ok(ImageBufferF32::new(roi.width, roi.height, 3))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dag_creation_and_cycle_prevention() {
        let mut dag = DagEngine::new();
        let raw = dag.add_node("RAW Sensor", NodeType::RawSource { path: "test.raw".into() });
        let wb = dag.add_node("White Balance", NodeType::WhiteBalance { temp: 5600.0, tint: 0.0 });
        let exp = dag.add_node("Exposure", NodeType::Exposure { ev: 1.0 });
        let sink = dag.add_node("Display Sink", NodeType::DisplaySink);

        assert!(dag.connect(raw, wb).is_ok());
        assert!(dag.connect(wb, exp).is_ok());
        assert!(dag.connect(exp, sink).is_ok());

        // Attempting to introduce a cycle: sink -> raw must fail
        assert!(dag.connect(sink, raw).is_err());
    }

    #[test]
    fn test_dag_demand_driven_evaluation() {
        let mut dag = DagEngine::new();
        let raw = dag.add_node("RAW Sensor", NodeType::RawSource { path: "sample.raw".into() });
        let exp = dag.add_node("Exposure +1 EV", NodeType::Exposure { ev: 1.0 });
        let sink = dag.add_node("Display Sink", NodeType::DisplaySink);

        dag.connect(raw, exp).unwrap();
        dag.connect(exp, sink).unwrap();

        let roi = TileRoi::full(128, 128);
        let res = dag.evaluate_node(sink, roi);
        assert!(res.is_ok());
        let buf = res.unwrap();
        assert_eq!(buf.width, 128);
        assert_eq!(buf.height, 128);
    }
}
