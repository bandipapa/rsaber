use std::cell::{Cell, RefCell};
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::Arc;

use hashbrown::HashMap;
use wgpu::{CommandEncoder, Texture, TextureView};

use crate::asset::AssetManagerRc;
use crate::output::{Frame, OutputDeviceRc};
use crate::util::StatsRc;

pub type RenderTextureNamedInputs<'a> = HashMap<&'a str, Option<RenderTextureIn>>;

pub trait RenderNodeInOut {
    type InOut;

    fn get_in_names() -> &'static [&'static str];
    fn build(ins: RenderTextureNamedInputs) -> Self::InOut;
    fn get_out(&self, out_name: &str) -> &RenderTextureOut;
}

#[expect(dead_code)]
pub trait RenderNodeBuild: RenderNodeInOut {
    fn build(asset_mgr: AssetManagerRc, output_device: OutputDeviceRc, stats: StatsRc, in_out: Self::InOut) -> Self;
}

pub trait RenderNodeBuildWithParam: RenderNodeInOut {
    type Param;

    fn build(asset_mgr: AssetManagerRc, output_device: OutputDeviceRc, stats: StatsRc, param: Self::Param, in_out: Self::InOut) -> Self;
}

pub trait RenderNodeExec {
    fn configure(&self, width: u32, height: u32);
    fn render(&self, encoder: &mut CommandEncoder, frame: &dyn Frame);
}

type RenderTextureInner = Rc<RefCell<Option<TextureView>>>;

pub struct RenderTextureIn {
    inner: RenderTextureInner,
}

impl RenderTextureIn {
    #[expect(dead_code)]
    fn new(inner: RenderTextureInner) -> Self {
        Self {
            inner,
        }
    }

    #[expect(dead_code)]
    pub fn get_view(&self) -> TextureView {
        self.inner.borrow().as_ref().expect("Texture is not set").clone() // TODO: avoid clone and return with &TextureView?
    }
}

#[derive(Clone)]
pub struct RenderTextureOut {
    inner: RenderTextureInner,
    final_flag: Cell<bool>,
}

impl RenderTextureOut {
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(None)),
            final_flag: Cell::new(false),
        }
    }

    pub fn set_texture<F: FnOnce() -> Texture>(&self, func: F) {
        // Implementation notes:
        // - Should be called by RenderNodeExec->configure to set the output
        //   texture view, which will be connected to input texture view(s).
        // - In case of final_flag, this method has no effect. It is used
        //   to allow node to directly render to the actual swapchain image,
        //   so we can avoid additional copy from node output to swapchain.

        if !self.final_flag.get() {
            let texture = func();
            let view = texture.create_view(&Default::default());
            self.inner.borrow_mut().replace(view);
        }
    }

    pub fn get_view(&self) -> TextureView {
        self.inner.borrow().as_ref().expect("Texture is not set").clone() // TODO: avoid clone and return with &TextureView?
    }

    fn set_final_flag(&self) {
        assert!(!self.final_flag.get());
        self.final_flag.set(true);
    }

    fn set_final_view(&self, view: TextureView) {
        assert!(self.final_flag.get());
        self.inner.borrow_mut().replace(view);
    }

    #[expect(dead_code)]
    fn get_inner(&self) -> RenderTextureInner {
        Rc::clone(&self.inner)
    }
}

pub struct RenderGraphBuilder {
    asset_mgr: AssetManagerRc,
    output_device: OutputDeviceRc,
    stats: StatsRc,
    nodes: RefCell<Vec<Rc<dyn RenderNodeExec>>>,
}

impl RenderGraphBuilder {
    pub fn new(asset_mgr: AssetManagerRc, output_device: OutputDeviceRc, stats: StatsRc) -> Self {
        Self {
            asset_mgr,
            output_device,
            stats,
            nodes: RefCell::new(Vec::new()),
        }
    }
    
    pub fn create_node<T: RenderNodeInOut + RenderNodeExec + 'static>(&self) -> RenderNodeBuilder<'_, T> {
        RenderNodeBuilder::new(self)
    }
    
    pub fn build(self, out_color: &RenderTextureOut) -> RenderGraph {
        RenderGraph::new(self.nodes.into_inner().into_boxed_slice(), out_color)
    }

    fn add_node(&self, node: Rc<dyn RenderNodeExec>) {
        self.nodes.borrow_mut().push(node);
    }    
}

pub struct RenderNodeBuilder<'a, T> {
    rgb: &'a RenderGraphBuilder,
    ins: RenderTextureNamedInputs<'a>,
    _phantom: PhantomData<T>,
}

impl<'a, T: RenderNodeInOut + RenderNodeExec + 'static> RenderNodeBuilder<'a, T> {
    fn new(rgb: &'a RenderGraphBuilder) -> Self {
        Self {
            rgb,
            ins: HashMap::from_iter(T::get_in_names().iter().map(|in_name| (*in_name, None))),
            _phantom: PhantomData,
        }
    }

    #[expect(dead_code)]
    pub fn connect(mut self, in_name: &str, out: &RenderTextureOut) -> Self {
        self.ins.get_mut(in_name).expect("No such input").replace(RenderTextureIn::new(out.get_inner()));
        self
    }

    #[expect(dead_code)]
    pub fn build(self) -> Rc<T> where T: RenderNodeBuild {
        let in_out = <T as RenderNodeInOut>::build(self.ins);
        let node = <T as RenderNodeBuild>::build(Arc::clone(&self.rgb.asset_mgr), Rc::clone(&self.rgb.output_device), Arc::clone(&self.rgb.stats), in_out);
        Self::add_node(self.rgb, node)
    }

    pub fn build_with_param(self, param: T::Param) -> Rc<T> where T: RenderNodeBuildWithParam {
        let in_out = <T as RenderNodeInOut>::build(self.ins);
        let node = <T as RenderNodeBuildWithParam>::build(Arc::clone(&self.rgb.asset_mgr), Rc::clone(&self.rgb.output_device), Arc::clone(&self.rgb.stats), param, in_out);
        Self::add_node(self.rgb, node)
    }

    fn add_node(rgb: &RenderGraphBuilder, node: T) -> Rc<T> {
        // Once the node is built, then its output(s) will become
        // accessible (see RenderNodeInOut->get_out).

        let node = Rc::new(node);
        rgb.add_node(Rc::clone(&node) as Rc<dyn RenderNodeExec>);
        node
    }
}

pub struct RenderGraph {
    nodes: Box<[Rc<dyn RenderNodeExec>]>,
    out_color: RenderTextureOut,
}

impl RenderGraph {
    // TODO: wgpu does the scheduling of render passes. If we switch to raw Vulkan API
    // in the future, we will have greater control over scheduling.

    fn new(nodes: Box<[Rc<dyn RenderNodeExec>]>, out_color: &RenderTextureOut) -> Self {
        out_color.set_final_flag();

        Self {
            nodes,
            out_color: out_color.clone(),
        }
    }

    pub fn configure(&self, width: u32, height: u32) {
        assert!(width > 0 && height > 0);

        // TODO: Would be nice if we can check out->in texture compatibility (e.g. format, dimensions,
        // layers, etc.). This is true for the final_flag as well.

        for node in &self.nodes {
            node.configure(width, height);
        }
    }

    pub fn render(&self, encoder: &mut CommandEncoder, frame: &dyn Frame) {
        let color_view = frame.get_color_view();
        self.out_color.set_final_view(color_view.clone());

        for node in &self.nodes {
            node.render(encoder, frame);
        }
    }
}
