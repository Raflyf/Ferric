use crate::{
    makepad_shader_compiler::generate_hlsl,
    makepad_math::*,
    os::{
        windows::win32_app::{TRUE, FALSE,},
        windows::win32_window::Win32Window,
    },
    texture::Texture,
    draw_list::DrawListId,
    event::WindowGeom,
    cx::Cx,
    draw_shader::CxDrawShaderMapping,
    pass::{PassClearColor, PassClearDepth, PassId},
    window::WindowId,
    texture::{ 
        TextureFormat,
        TexturePixel,
        TextureId,
        CxTexture
    },  
    windows::{
        core::{
            PCSTR,
            //ComInterface,
            Interface,
        },
        Win32::{
            Foundation::{
                HINSTANCE,
                HANDLE,
                S_FALSE,
            },
            UI::WindowsAndMessaging::{
                GetWindowLongPtrW,
                SetWindowLongPtrW,
                GWL_EXSTYLE,
                WS_EX_NOREDIRECTIONBITMAP,
            },
            Graphics::{
                Direct3D11::{
                    D3D11_BIND_INDEX_BUFFER,
                    D3D11_BIND_VERTEX_BUFFER,
                    D3D11_VIEWPORT,
                    D3D11_BUFFER_DESC,
                    D3D11_USAGE_DEFAULT,
                    D3D11_BIND_CONSTANT_BUFFER,
                    D3D11_RESOURCE_MISC_FLAG,
                    D3D11_SUBRESOURCE_DATA,
                    D3D11_CREATE_DEVICE_FLAG,
                    D3D11_SDK_VERSION,
                    D3D11_BIND_FLAG,
                    D3D11_BIND_SHADER_RESOURCE,
                    D3D11_TEXTURE2D_DESC,
                    D3D11_BIND_RENDER_TARGET,
                    D3D11_BIND_DEPTH_STENCIL,
                    D3D11_DEPTH_STENCIL_DESC,
                    D3D11_DEPTH_WRITE_MASK_ALL,
                    D3D11_COMPARISON_LESS_EQUAL,
                    D3D11_DEPTH_STENCILOP_DESC,
                    D3D11_STENCIL_OP_REPLACE,
                    D3D11_COMPARISON_ALWAYS,
                    D3D11_DEPTH_STENCIL_VIEW_DESC,
                    D3D11_DSV_DIMENSION_TEXTURE2D,
                    D3D11_CLEAR_DEPTH,
                    D3D11_CLEAR_STENCIL,
                    D3D11_BLEND_DESC,
                    D3D11_RENDER_TARGET_BLEND_DESC,
                    D3D11_BLEND_ONE,
                    D3D11_BLEND_INV_SRC_ALPHA,
                    D3D11_BLEND_OP_ADD,
                    D3D11_COLOR_WRITE_ENABLE_ALL,
                    D3D11_RASTERIZER_DESC,
                    D3D11_CULL_NONE,
                    D3D11_FILL_SOLID,
                    D3D11_SAMPLER_DESC,
                    D3D11_FILTER_MIN_MAG_MIP_LINEAR,
                    D3D11_TEXTURE_ADDRESS_WRAP,
                    D3D11_COMPARISON_NEVER,
                    D3D11_USAGE_IMMUTABLE,
                    D3D11_INPUT_ELEMENT_DESC,
                    D3D11_INPUT_PER_VERTEX_DATA,
                    D3D11_INPUT_PER_INSTANCE_DATA,
                    D3D11_MAPPED_SUBRESOURCE,
                    D3D11_USAGE_DYNAMIC,
                    D3D11_CPU_ACCESS_WRITE,
                    D3D11_MAP_WRITE_DISCARD,
                    D3D11_QUERY_DESC,
                    D3D11_QUERY_EVENT,
                    ID3D11Device,
                    ID3D11DeviceContext,
                    ID3D11RenderTargetView,
                    ID3D11Texture2D,
                    ID3D11ShaderResourceView,
                    ID3D11DepthStencilView,
                    ID3D11BlendState,
                    ID3D11RasterizerState,
                    ID3D11DepthStencilState,
                    ID3D11SamplerState,
                    ID3D11PixelShader,
                    ID3D11VertexShader,
                    ID3D11InputLayout,
                    ID3D11Buffer,
                    D3D11CreateDevice,
                    ID3D11Resource,
                    ID3D11Query,
                },
                Direct3D::{
                    Fxc::D3DCompile,
                    ID3DBlob,
                    D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST,
                    D3D_DRIVER_TYPE_UNKNOWN,
                    D3D_DRIVER_TYPE_WARP,
                    D3D_FEATURE_LEVEL_11_0,
                    D3D_FEATURE_LEVEL_10_1,
                    D3D_FEATURE_LEVEL_10_0,
                },
                DirectComposition::{
                    DCompositionCreateDevice,
                    IDCompositionDevice,
                    IDCompositionTarget,
                    IDCompositionVisual,
                },
                Dxgi::{
                    IDXGIAdapter1,
                    IDXGIDevice,
                    IDXGIFactory2,
                    IDXGIResource,
                    IDXGISwapChain1,
                    CreateDXGIFactory2,
                    DXGI_ADAPTER_DESC1,
                    DXGI_ADAPTER_FLAG_SOFTWARE,
                    DXGI_SWAP_CHAIN_DESC1,
                    DXGI_USAGE_RENDER_TARGET_OUTPUT,
                    DXGI_SCALING_NONE,
                    DXGI_SCALING_STRETCH,
                    DXGI_SWAP_EFFECT_FLIP_DISCARD,
                    DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                    DXGI_RGBA,
                    Common::{
                        DXGI_FORMAT,
                        DXGI_ALPHA_MODE_IGNORE,
                        DXGI_ALPHA_MODE_PREMULTIPLIED,
                        DXGI_FORMAT_R8_UNORM, 
                        DXGI_FORMAT_R8G8_UNORM,
                        DXGI_FORMAT_B8G8R8A8_UNORM,
                        DXGI_SAMPLE_DESC,
                        DXGI_FORMAT_R32G32B32A32_FLOAT,
                        DXGI_FORMAT_R16_FLOAT, 
                        //DXGI_FORMAT_D32_FLOAT_S8X 24_UINT,
                        DXGI_FORMAT_D32_FLOAT,
                        DXGI_FORMAT_R32_UINT,
                        DXGI_FORMAT_R32_FLOAT,
                        DXGI_FORMAT_R32G32_FLOAT,
                        DXGI_FORMAT_R32G32B32_FLOAT,
                    },
                },
            },
        },
    },
};

impl Cx {
    
    fn render_view(
        &mut self,
        pass_id: PassId,
        draw_list_id: DrawListId,
        zbias: &mut f32,
        zbias_step: f32,
        d3d11_cx: &D3d11Cx
    ) {
        // tad ugly otherwise the borrow checker locks 'self' and we can't recur
        let draw_items_len = self.draw_lists[draw_list_id].draw_items.len();
        
        {
            let draw_list = &mut self.draw_lists[draw_list_id];
            draw_list.os.draw_list_uniforms.update_with_f32_constant_data(d3d11_cx, draw_list.draw_list_uniforms.as_slice());
        }
        
        for draw_item_id in 0..draw_items_len {
            if let Some(sub_list_id) = self.draw_lists[draw_list_id].draw_items[draw_item_id].kind.sub_list() {
                self.render_view(
                    pass_id,
                    sub_list_id,
                    zbias,
                    zbias_step,
                    d3d11_cx,
                );
            }
            else {
                let draw_list = &mut self.draw_lists[draw_list_id];
                let draw_item = &mut draw_list.draw_items[draw_item_id];
                let draw_call = if let Some(draw_call) = draw_item.kind.draw_call_mut() {
                    draw_call
                } else {
                    continue;
                };
                let sh = &self.draw_shaders[draw_call.draw_shader.draw_shader_id];
                if sh.os_shader_id.is_none() { // shader didnt compile somehow
                    continue;
                }
                if sh.mapping.uses_time {
                    self.demo_time_repaint = true;
                }
                let shp = &self.draw_shaders.os_shaders[sh.os_shader_id.unwrap()];

                if draw_call.instance_dirty {
                    draw_call.instance_dirty = false;
                    if draw_item.instances.as_ref().unwrap().len() == 0 {
                        continue;
                    }
                    // update the instance buffer data
                    draw_item.os.inst_vbuf.update_with_f32_vertex_data(d3d11_cx, draw_item.instances.as_ref().unwrap());
                }
                
                // update the zbias uniform if we have it.
                draw_call.draw_call_uniforms.set_zbias(*zbias);
                *zbias += zbias_step;
                
                if draw_call.uniforms_dirty {
                    draw_call.uniforms_dirty = false;
                    draw_item.os.draw_call_uniforms.update_with_f32_constant_data(d3d11_cx, draw_call.draw_call_uniforms.as_slice());
                    
                }
                if draw_call.user_uniforms.len() != 0 {
                    draw_item.os.user_uniforms.update_with_f32_constant_data(d3d11_cx, &mut draw_call.user_uniforms);
                }
                
                let instances = (draw_item.instances.as_ref().unwrap().len() / sh.mapping.instances.total_slots) as u64;
                
                if instances == 0 {
                    continue;
                }
                
                let geometry_id = if let Some(geometry_id) = draw_call.geometry_id {geometry_id}
                else {
                    continue;
                };
                
                let geometry = &mut self.geometries[geometry_id];
                
                if geometry.dirty {
                    geometry.os.geom_ibuf.update_with_u32_index_data(d3d11_cx, &geometry.indices);
                    geometry.os.geom_vbuf.update_with_f32_vertex_data(d3d11_cx, &geometry.vertices);
                    geometry.dirty = false;
                }
                
                unsafe {
                    d3d11_cx.context.VSSetShader(&shp.vertex_shader, None);
                    d3d11_cx.context.PSSetShader(&shp.pixel_shader, None);
                    d3d11_cx.context.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
                    d3d11_cx.context.IASetInputLayout(&shp.input_layout);
                    
                    let geom_ibuf = geometry.os.geom_ibuf.buffer.as_ref().unwrap();
                    d3d11_cx.context.IASetIndexBuffer(geom_ibuf, DXGI_FORMAT_R32_UINT, 0);
                    
                    let geom_slots = sh.mapping.geometries.total_slots;
                    let inst_slots = sh.mapping.instances.total_slots;
                    let strides = [(geom_slots * 4) as u32, (inst_slots * 4) as u32];
                    let offsets = [0u32, 0u32];
                    let buffers = [geometry.os.geom_vbuf.buffer.clone(), draw_item.os.inst_vbuf.buffer.clone()];
                    d3d11_cx.context.IASetVertexBuffers(0, 2, Some(buffers.as_ptr()), Some(strides.as_ptr()), Some(offsets.as_ptr()));
                    
                    fn buffer_slot(d3d11_cx: &D3d11Cx, index: u32, buffer: &Option<ID3D11Buffer>) {
                        unsafe {
                            if let Some(buffer) = buffer.clone() {
                                let buffers = [Some(buffer)];
                                d3d11_cx.context.VSSetConstantBuffers(index, Some(&buffers));
                                d3d11_cx.context.PSSetConstantBuffers(index, Some(&buffers));
                            }
                            else {
                                d3d11_cx.context.VSSetConstantBuffers(index, None);
                                d3d11_cx.context.PSSetConstantBuffers(index, None);
                            }
                        }
                    }
                    buffer_slot(d3d11_cx, 0, &shp.live_uniforms.buffer);
                    buffer_slot(d3d11_cx, 1, &shp.const_table_uniforms.buffer);
                    buffer_slot(d3d11_cx, 2, &draw_item.os.draw_call_uniforms.buffer);
                    buffer_slot(d3d11_cx, 3, &self.passes[pass_id].os.pass_uniforms.buffer);
                    buffer_slot(d3d11_cx, 4, &draw_list.os.draw_list_uniforms.buffer);
                    buffer_slot(d3d11_cx, 5, &draw_item.os.user_uniforms.buffer);
                }
                
                for i in 0..sh.mapping.textures.len() {
                    
                    let texture_id = if let Some(texture) = &draw_call.texture_slots[i] {
                        texture.texture_id()
                    } else {
                        continue;
                    };
                    
                    let cxtexture = &mut self.textures[texture_id];
                    
                    if cxtexture.format.is_shared() {
                        cxtexture.update_shared_texture(
                            &d3d11_cx.device,
                        );
                    }
                    else if cxtexture.format.is_vec(){
                        cxtexture.update_vec_texture(
                            d3d11_cx,
                        );
                    }
                    unsafe {
                        if let Some(sr) = &cxtexture.os.shader_resource_view {
                            d3d11_cx.context.PSSetShaderResources(i as u32, Some(&[Some(sr.clone())]));
                            d3d11_cx.context.VSSetShaderResources(i as u32, Some(&[Some(sr.clone())]));
                        }
                        else {
                            d3d11_cx.context.PSSetShaderResources(i as u32, None);
                            d3d11_cx.context.VSSetShaderResources(i as u32, None);
                        }
                    }
                }
                //if self.passes[pass_id].debug{
                // println!("DRAWING {} {}", geometry.indices.len(), instances);
                //}
                unsafe {
                    d3d11_cx.context.DrawIndexedInstanced(
                        geometry.indices.len() as u32,
                        instances as u32,
                        0,
                        0,
                        0
                    )
                };
            }
        }
    }

    pub fn get_shared_handle(&self, _texture: &Texture) -> HANDLE {
        self.textures[_texture.texture_id()].os.shared_handle
    }

    pub fn setup_pass_render_targets(&mut self, pass_id: PassId, first_target: &Option<ID3D11RenderTargetView >, d3d11_cx: &D3d11Cx) {
        
        let dpi_factor = self.passes[pass_id].dpi_factor.unwrap();
        
        let pass_rect = self.get_pass_rect(pass_id, dpi_factor).unwrap();
        self.passes[pass_id].set_ortho_matrix(pass_rect.pos, pass_rect.size);
        self.passes[pass_id].paint_dirty = false;

        self.passes[pass_id].set_dpi_factor(dpi_factor);
        
        let viewport = D3D11_VIEWPORT {
            Width: (pass_rect.size.x * dpi_factor) as f32,
            Height: (pass_rect.size.y * dpi_factor) as f32,
            MinDepth: 0.,
            MaxDepth: 1.,
            TopLeftX: 0.0,
            TopLeftY: 0.0
        };
        unsafe {
            d3d11_cx.context.RSSetViewports(Some(&[viewport]));
        }
        if viewport.Width < 1.0 || viewport.Height < 1.0{
            return
        }
        // set up the color texture array
        let mut color_textures = Vec::<Option<ID3D11RenderTargetView>>::new();
        
        if let Some(render_target) = first_target {
            color_textures.push(Some(render_target.clone()));
            let color = self.passes[pass_id].clear_color;
            let color = [color.x, color.y, color.z, color.w];
            unsafe {d3d11_cx.context.ClearRenderTargetView(first_target.as_ref().unwrap(), &color)}
        }
        else {
            for color_texture in self.passes[pass_id].color_textures.iter() {
                let cxtexture = &mut self.textures[color_texture.texture.texture_id()];
                let size = pass_rect.size * dpi_factor;
                cxtexture.update_render_target(d3d11_cx, size.x as usize, size.y as usize);
                let is_initial = cxtexture.take_initial();
                let render_target = cxtexture.os.render_target_view.clone();
                color_textures.push(Some(render_target.clone().unwrap()));
                // possibly clear it
                match color_texture.clear_color {
                    PassClearColor::InitWith(color) => {
                        if is_initial {
                            let color = [color.x, color.y, color.z, color.w];
                            unsafe {d3d11_cx.context.ClearRenderTargetView(render_target.as_ref().unwrap(), &color)}
                        }
                    },
                    PassClearColor::ClearWith(color) => {
                        let color = [color.x, color.y, color.z, color.w];
                        unsafe {d3d11_cx.context.ClearRenderTargetView(render_target.as_ref().unwrap(), &color)}
                    }
                }
            }
        }
        
        // D3D11 backend renders 2D passes with OMSetRenderTargets(..., None); skip allocating and
        // clearing an unbound fullscreen D32 depth buffer (~8.3 MB VRAM/RAM saved at 1080p).
        unsafe {d3d11_cx.context.OMSetRenderTargets(
            Some(&color_textures),
            None
        )}
        
        // create depth, blend and raster states
        self.passes[pass_id].os.set_states(d3d11_cx);
        
        let cxpass = &mut self.passes[pass_id];
        
        cxpass.os.pass_uniforms.update_with_f32_constant_data(&d3d11_cx, cxpass.pass_uniforms.as_slice());
    }
    
    pub fn draw_pass_to_window(&mut self, pass_id: PassId, vsync: bool, d3d11_window: &mut D3d11Window, d3d11_cx: &D3d11Cx) {
        if d3d11_window.render_target_view.is_none() {
            return;
        }
        // let time1 = Cx::profile_time_ns();
        let draw_list_id = self.passes[pass_id].main_draw_list_id.unwrap();
        
        self.setup_pass_render_targets(pass_id, &d3d11_window.render_target_view, d3d11_cx);
        
        let mut zbias = 0.0;
        let zbias_step = self.passes[pass_id].zbias_step;
        
        self.render_view(
            pass_id,
            draw_list_id,
            &mut zbias,
            zbias_step,
            d3d11_cx
        );
        d3d11_window.present(vsync);
        if d3d11_window.first_draw {
            if let Some(dcomp_dev) = &d3d11_window.dcomp_device {
                let _ = unsafe { dcomp_dev.Commit() };
            }
            d3d11_window.win32_window.show();
            d3d11_window.win32_window.refresh_visible_acrylic_backdrop();
            if let Some(dcomp_dev) = &d3d11_window.dcomp_device {
                let _ = unsafe { dcomp_dev.Commit() };
            }
            d3d11_window.first_draw = false;
        }
        //println!("{}", (Cx::profile_time_ns() - time1)as f64 / 1000.0);
    }
    
    pub fn draw_pass_to_texture(&mut self, pass_id: PassId,  d3d11_cx: &D3d11Cx,texture_id: Option<TextureId>) {
        // let time1 = Cx::profile_time_ns();
        let draw_list_id = self.passes[pass_id].main_draw_list_id.unwrap();
        
        if let Some(texture_id) = texture_id{
            let render_target_view = self.textures[texture_id].os.render_target_view.clone();
            self.setup_pass_render_targets(pass_id, &render_target_view, d3d11_cx);
        }
        else{
            self.setup_pass_render_targets(pass_id, &None, d3d11_cx);
        }
            
        let mut zbias = 0.0;
        let zbias_step = self.passes[pass_id].zbias_step;
        self.render_view(
            pass_id,
            draw_list_id,
            &mut zbias,
            zbias_step,
            &d3d11_cx,
        );
    }
    
    pub (crate) fn hlsl_compile_shaders(&mut self, d3d11_cx: &D3d11Cx) {
        for draw_shader_ptr in &self.draw_shaders.compile_set {
            if let Some(item) = self.draw_shaders.ptr_to_item.get(&draw_shader_ptr) {
                let cx_shader = &mut self.draw_shaders.shaders[item.draw_shader_id];
                let draw_shader_def = self.shader_registry.draw_shader_defs.get(&draw_shader_ptr);
                let hlsl = generate_hlsl::generate_shader(
                    draw_shader_def.as_ref().unwrap(),
                    &cx_shader.mapping.const_table,
                    &self.shader_registry
                );
                
                if cx_shader.mapping.flags.debug {
                    crate::log!("{}", hlsl);
                }
                // lets see if we have the shader already
                for (index, ds) in self.draw_shaders.os_shaders.iter().enumerate() {
                    if ds.hlsl == hlsl {
                        cx_shader.os_shader_id = Some(index);
                        break;
                    }
                }
                if cx_shader.os_shader_id.is_none() {
                    if let Some(shp) = CxOsDrawShader::new(d3d11_cx, hlsl, &cx_shader.mapping) {
                        cx_shader.os_shader_id = Some(self.draw_shaders.os_shaders.len());
                        self.draw_shaders.os_shaders.push(shp);
                    }
                }
            }
        }
        self.draw_shaders.compile_set.clear();
    }

    pub fn share_texture_for_presentable_image(
        &mut self,
        texture: &Texture,
    ) -> crate::cx_stdin::SharedPresentableImageOsHandle {
        let cxtexture = &mut self.textures[texture.texture_id()];
        cxtexture.update_shared_texture(self.os.d3d11_device.as_ref().unwrap());
        cxtexture.os.shared_handle.0 as u64
    }
}

fn texture_pixel_to_dx11_pixel(pix:&TexturePixel)->DXGI_FORMAT{
    match pix{
        TexturePixel::BGRAu8 => DXGI_FORMAT_B8G8R8A8_UNORM,
        TexturePixel::RGBAf16 => DXGI_FORMAT_R16_FLOAT,
        TexturePixel::RGBAf32 => DXGI_FORMAT_R32G32B32A32_FLOAT,
        TexturePixel::Ru8  => DXGI_FORMAT_R8_UNORM,
        TexturePixel::RGu8  => DXGI_FORMAT_R8G8_UNORM,
        TexturePixel::Rf32  => DXGI_FORMAT_R32_FLOAT,
        TexturePixel::D32 => DXGI_FORMAT_D32_FLOAT,
    }   
}

pub struct D3d11Window {
    pub window_id: WindowId,
    pub is_in_resize: bool,
    pub window_geom: WindowGeom,
    pub win32_window: Box<Win32Window>,
    pub render_target_view: Option<ID3D11RenderTargetView >,
    pub swap_texture: Option<ID3D11Texture2D >,
    pub alloc_size: DVec2,
    pub first_draw: bool,
    pub swap_chain: IDXGISwapChain1,
    pub dcomp_device: Option<IDCompositionDevice>,
    pub dcomp_target: Option<IDCompositionTarget>,
    pub dcomp_visual: Option<IDCompositionVisual>,
}

impl D3d11Window {
    pub fn new(window_id: WindowId, d3d11_cx: &D3d11Cx, inner_size: DVec2, position: Option<DVec2>, title: &str) -> D3d11Window {

        // create window, and then initialize it; this is needed because
        // GWLP_USERDATA needs to reference a stable and existing window
        let mut win32_window = Box::new(Win32Window::new(window_id, title, position));
        win32_window.init(inner_size);
        
        let wg = win32_window.get_window_geom();
        let sc_w = ((wg.inner_size.x * wg.dpi_factor).round() as u32).max(1);
        let sc_h = ((wg.inner_size.y * wg.dpi_factor).round() as u32).max(1);

        let comp_desc = DXGI_SWAP_CHAIN_DESC1 {
            AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
            BufferCount: 2,
            Width: sc_w,
            Height: sc_h,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            Flags: 0,
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            SampleDesc: DXGI_SAMPLE_DESC {Count: 1, Quality: 0,},
            Scaling: DXGI_SCALING_STRETCH,
            Stereo: FALSE,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
        };
        
        unsafe {
            fn try_create_dcomp_swapchain(
                d3d11_cx: &D3d11Cx,
                hwnd: crate::windows::Win32::Foundation::HWND,
                comp_desc: &DXGI_SWAP_CHAIN_DESC1,
            ) -> crate::windows::core::Result<(
                IDXGISwapChain1,
                IDCompositionDevice,
                IDCompositionTarget,
                IDCompositionVisual,
            )> {
                unsafe {
                    let dxgi_device: IDXGIDevice = d3d11_cx.device.cast()?;
                    let dcomp_dev: IDCompositionDevice = DCompositionCreateDevice(&dxgi_device)?;
                    let target = dcomp_dev.CreateTargetForHwnd(hwnd, TRUE)?;
                    let visual = dcomp_dev.CreateVisual()?;
                    let sc = d3d11_cx.factory.CreateSwapChainForComposition(
                        &d3d11_cx.device,
                        comp_desc,
                        None,
                    )?;
                    visual.SetContent(&sc)?;
                    target.SetRoot(&visual)?;
                    dcomp_dev.Commit()?;
                    Ok((sc, dcomp_dev, target, visual))
                }
            }

            let mut dcomp_device_opt: Option<IDCompositionDevice> = None;
            let mut dcomp_target_opt: Option<IDCompositionTarget> = None;
            let mut dcomp_visual_opt: Option<IDCompositionVisual> = None;

            let swap_chain = if let Ok((sc, dcomp_dev, target, visual)) =
                try_create_dcomp_swapchain(d3d11_cx, win32_window.hwnd, &comp_desc)
            {
                dcomp_device_opt = Some(dcomp_dev);
                dcomp_target_opt = Some(target);
                dcomp_visual_opt = Some(visual);
                sc
            } else {
                // Fallback to HWND swapchain: strip WS_EX_NOREDIRECTIONBITMAP so DXGI can present cleanly.
                let ex_style = GetWindowLongPtrW(win32_window.hwnd, GWL_EXSTYLE);
                if (ex_style & (WS_EX_NOREDIRECTIONBITMAP.0 as isize)) != 0 {
                    SetWindowLongPtrW(
                        win32_window.hwnd,
                        GWL_EXSTYLE,
                        ex_style & !(WS_EX_NOREDIRECTIONBITMAP.0 as isize),
                    );
                }
                let sc_desc = DXGI_SWAP_CHAIN_DESC1 {
                    AlphaMode: DXGI_ALPHA_MODE_IGNORE,
                    BufferCount: 2,
                    Width: sc_w,
                    Height: sc_h,
                    Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    Flags: 0,
                    BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                    SampleDesc: DXGI_SAMPLE_DESC {Count: 1, Quality: 0,},
                    Scaling: DXGI_SCALING_NONE,
                    Stereo: FALSE,
                    SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
                };
                let sc = d3d11_cx.factory.CreateSwapChainForHwnd(
                    &d3d11_cx.device,
                    win32_window.hwnd,
                    &sc_desc,
                    None,
                    None,
                ).unwrap();
                let _ = sc.SetBackgroundColor(&mut DXGI_RGBA {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 1.0
                });
                sc
            };
            
            let swap_texture = swap_chain.GetBuffer(0).unwrap();
            let mut render_target_view = None;
            d3d11_cx.device.CreateRenderTargetView(&swap_texture, None, Some(&mut render_target_view)).unwrap();
            D3d11Window {
                first_draw: true,
                is_in_resize: false,
                window_id: window_id,
                alloc_size: wg.inner_size,
                window_geom: wg,
                win32_window: win32_window,
                swap_texture: Some(swap_texture),
                render_target_view: render_target_view,
                swap_chain: swap_chain,
                dcomp_device: dcomp_device_opt,
                dcomp_target: dcomp_target_opt,
                dcomp_visual: dcomp_visual_opt,
            }
        }
    }
    
    pub fn start_resize(&mut self) {
        self.is_in_resize = true;
    }
    
    // switch back to swapchain
    pub fn stop_resize(&mut self) {
        self.is_in_resize = false;
        self.alloc_size = DVec2::default();
    }
    
    
    pub fn resize_buffers(&mut self, d3d11_cx: &D3d11Cx) {
        if self.window_geom.inner_size.x <= 0.0 || self.window_geom.inner_size.y <= 0.0 {
            // Window minimized (0x0 client rect): keep existing swapchain intact so taskbar restore
            // presents immediately without reallocating or flashing the DWM Acrylic surface.
            return;
        }
        if self.alloc_size == self.window_geom.inner_size && self.render_target_view.is_some() {
            return;
        }
        self.alloc_size = self.window_geom.inner_size;
        unsafe {
            // DXGI requires all backbuffer references to be unbound from the immediate context
            // before ResizeBuffers is invoked, otherwise ResizeBuffers can fail with INVALID_CALL.
            d3d11_cx.context.OMSetRenderTargets(None, None);
            d3d11_cx.context.Flush();
        }
        self.swap_texture = None;
        self.render_target_view = None;
        
        unsafe {
            let wg = &self.window_geom;
            let sc_w = ((wg.inner_size.x * wg.dpi_factor).round() as u32).max(1);
            let sc_h = ((wg.inner_size.y * wg.dpi_factor).round() as u32).max(1);
            if self
                .swap_chain
                .ResizeBuffers(2, sc_w, sc_h, DXGI_FORMAT_B8G8R8A8_UNORM, 0)
                .is_err()
            {
                // Device reset / TDR / monitor hotplug: avoid aborting the process; retry next resize.
                self.alloc_size = DVec2::default();
                return;
            }

            let Ok(swap_texture) = self.swap_chain.GetBuffer(0) else {
                self.alloc_size = DVec2::default();
                return;
            };
            let mut render_target_view = None;
            if d3d11_cx
                .device
                .CreateRenderTargetView(&swap_texture, None, Some(&mut render_target_view))
                .is_err()
            {
                self.alloc_size = DVec2::default();
                return;
            }

            self.swap_texture = Some(swap_texture);
            self.render_target_view = render_target_view;
            if let Some(dcomp_dev) = &self.dcomp_device {
                let _ = dcomp_dev.Commit();
            }
        }
    }
    
    pub fn present(&mut self, vsync: bool) {
        unsafe {
            let _ = self.swap_chain.Present(if vsync { 1 } else { 0 }, 0);
        }
    }
    
}

#[derive(Clone)]
pub struct D3d11Cx {
    pub device: ID3D11Device,
    pub context: ID3D11DeviceContext,
    pub query: ID3D11Query,
    pub factory: IDXGIFactory2,
}

impl D3d11Cx {
    pub fn new() -> D3d11Cx {
        unsafe {
            let factory: IDXGIFactory2 = CreateDXGIFactory2(0).unwrap();
            let feature_levels = [
                D3D_FEATURE_LEVEL_11_0,
                D3D_FEATURE_LEVEL_10_1,
                D3D_FEATURE_LEVEL_10_0,
            ];

            // Strict minimum-RAM/power hardware policy: NEVER wake a discrete Dedicated GPU when
            // an Integrated GPU is present (avoids +200 MB discrete driver commit and battery draw).
            // 1. Check Adapter[0] first; if it is already an Integrated GPU (<1 GiB dedicated VRAM),
            //    stop immediately without even enumerating secondary discrete adapters.
            // 2. If Adapter[0] is a discrete GPU (>=1 GiB), check if a low-power Integrated GPU
            //    exists on another slot and prefer it; otherwise use Adapter[0] (dGPU-only desktop).
            // 3. Fall back cleanly to CPU WARP only if no hardware D3D11 adapter succeeds.
            const IGPU_VRAM_CEILING: usize = 1024 * 1024 * 1024;
            let mut candidates: Vec<IDXGIAdapter1> = Vec::new();
            if let Ok(first) = factory.EnumAdapters1(0) {
                let mut desc0 = DXGI_ADAPTER_DESC1::default();
                let first_is_sw = first.GetDesc1(&mut desc0).is_ok()
                    && (desc0.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0;
                if !first_is_sw && desc0.DedicatedVideoMemory < IGPU_VRAM_CEILING {
                    candidates.push(first);
                } else {
                    let mut igpus: Vec<IDXGIAdapter1> = Vec::new();
                    let mut others: Vec<IDXGIAdapter1> = Vec::new();
                    if !first_is_sw {
                        others.push(first);
                    }
                    for idx in 1..8u32 {
                        let Ok(adp) = factory.EnumAdapters1(idx) else {
                            break;
                        };
                        let mut d = DXGI_ADAPTER_DESC1::default();
                        if adp.GetDesc1(&mut d).is_err() {
                            continue;
                        }
                        if (d.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0 {
                            continue;
                        }
                        if d.DedicatedVideoMemory < IGPU_VRAM_CEILING {
                            igpus.push(adp);
                        } else {
                            others.push(adp);
                        }
                    }
                    candidates.extend(igpus);
                    candidates.extend(others);
                }
            }

            let mut selected: Option<(ID3D11Device, ID3D11DeviceContext)> = None;
            for adapter in &candidates {
                let mut device: Option<ID3D11Device> = None;
                let mut context: Option<ID3D11DeviceContext> = None;
                if D3D11CreateDevice(
                    adapter,
                    D3D_DRIVER_TYPE_UNKNOWN,
                    HINSTANCE(0),
                    D3D11_CREATE_DEVICE_FLAG(0),
                    Some(&feature_levels),
                    D3D11_SDK_VERSION,
                    Some(&mut device),
                    None,
                    Some(&mut context),
                )
                .is_ok()
                {
                    if let (Some(dev), Some(ctx)) = (device, context) {
                        selected = Some((dev, ctx));
                        break;
                    }
                }
            }

            let (device, context) = if let Some(ok) = selected {
                ok
            } else {
                let mut device: Option<ID3D11Device> = None;
                let mut context: Option<ID3D11DeviceContext> = None;
                D3D11CreateDevice(
                    None,
                    D3D_DRIVER_TYPE_WARP,
                    HINSTANCE(0),
                    D3D11_CREATE_DEVICE_FLAG(0),
                    Some(&feature_levels),
                    D3D11_SDK_VERSION,
                    Some(&mut device),
                    None,
                    Some(&mut context),
                )
                .unwrap();
                (device.unwrap(), context.unwrap())
            };

            let mut query: Option<ID3D11Query> = None;
            device
                .CreateQuery(
                    &D3D11_QUERY_DESC {
                        Query: D3D11_QUERY_EVENT,
                        MiscFlags: 0,
                    },
                    Some(&mut query),
                )
                .unwrap();

            let query = query.unwrap();

            D3d11Cx {
                device,
                context,
                factory,
                query,
            }
        }
    }

    pub fn start_querying(&self) {

        // QUERY_EVENT signals when rendering is complete
        unsafe { self.context.End(&self.query) };
    }

    pub fn is_gpu_done(&self) -> bool {
        let hresult = unsafe { (Interface::vtable(&self.context).GetData)(Interface::as_raw(&self.context),Interface::as_raw(&self.query),std::ptr::null_mut(),0,0) };
        hresult != S_FALSE
    }
}

#[derive(Clone, Default)]
pub struct CxOsDrawList {
    pub draw_list_uniforms: D3d11Buffer
}

#[derive(Default, Clone)]
pub struct CxOsDrawCall {
    pub draw_call_uniforms: D3d11Buffer,
    pub user_uniforms: D3d11Buffer,
    pub inst_vbuf: D3d11Buffer
}

#[derive(Default, Clone)]
pub struct D3d11Buffer {
    pub last_size: usize,
    pub cap_slots: usize,
    pub last_cb_u32: Vec<u32>,
    pub buffer: Option<ID3D11Buffer>
}

impl D3d11Buffer {
    fn upload_mapped(&self, d3d11_cx: &D3d11Cx, len_slots: usize, data: *const std::ffi::c_void) -> bool {
        let Some(buf) = self.buffer.as_ref() else {
            return false;
        };
        let Some(byte_len) = len_slots.checked_mul(4) else {
            return false;
        };
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        let p_mapped: *mut _ = &mut mapped;
        unsafe {
            if d3d11_cx
                .context
                .Map(buf, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(p_mapped))
                .is_err()
            {
                return false;
            }
            if !mapped.pData.is_null() {
                std::ptr::copy_nonoverlapping(data, mapped.pData, byte_len);
            }
            d3d11_cx.context.Unmap(buf, 0);
            !mapped.pData.is_null()
        }
    }

    fn create_buffer_or_update(&mut self, d3d11_cx: &D3d11Cx, buffer_desc: &D3D11_BUFFER_DESC, sub_data: &D3D11_SUBRESOURCE_DATA, len_slots: usize, data: *const std::ffi::c_void) -> bool {
        if len_slots == 0 {
            return true;
        }
        let is_cb = (buffer_desc.BindFlags & (D3D11_BIND_CONSTANT_BUFFER.0 as u32)) != 0;
        if is_cb {
            if self.buffer.is_some() && self.last_size == len_slots {
                return self.upload_mapped(d3d11_cx, len_slots, data);
            } else {
                let mut new_buf: Option<ID3D11Buffer> = None;
                let ok = unsafe {
                    d3d11_cx
                        .device
                        .CreateBuffer(buffer_desc, Some(sub_data), Some(&mut new_buf))
                        .is_ok()
                };
                if ok && new_buf.is_some() {
                    self.buffer = new_buf;
                    self.last_size = len_slots;
                    self.cap_slots = len_slots;
                    return true;
                }
                return false;
            }
        }
        // Vertex/index buffers: reuse existing dynamic ID3D11Buffer across small length changes
        // (e.g. text/lyrics glyph counts) to avoid Intel D3D11 driver CreateBuffer/GC RAM spikes.
        if self.buffer.is_some()
            && len_slots <= self.cap_slots
            && self.cap_slots <= len_slots.saturating_mul(4).max(256)
        {
            if self.upload_mapped(d3d11_cx, len_slots, data) {
                self.last_size = len_slots;
                return true;
            }
            return false;
        } else {
            let Some(cap_slots) = len_slots.checked_next_power_of_two().map(|c| c.max(64)) else {
                return false;
            };
            let Some(byte_width) = cap_slots
                .checked_mul(4)
                .and_then(|b| u32::try_from(b).ok())
            else {
                return false;
            };
            let mut alloc_desc = *buffer_desc;
            alloc_desc.ByteWidth = byte_width;
            let mut new_buf: Option<ID3D11Buffer> = None;
            let ok = unsafe {
                d3d11_cx
                    .device
                    .CreateBuffer(&alloc_desc, None, Some(&mut new_buf))
                    .is_ok()
            };
            if ok && new_buf.is_some() {
                // Assigning replaces and Release()s the previous ID3D11Buffer COM object cleanly.
                self.buffer = new_buf;
                self.cap_slots = cap_slots;
                if self.upload_mapped(d3d11_cx, len_slots, data) {
                    self.last_size = len_slots;
                    return true;
                }
                self.last_size = 0;
            }
            return false;
        }
    }

    pub fn update_with_data(&mut self, d3d11_cx: &D3d11Cx, bind_flags: D3D11_BIND_FLAG, len_slots: usize, data: *const std::ffi::c_void) {
        if len_slots == 0 {
            return;
        }
        let Some(byte_width) = len_slots
            .checked_mul(4)
            .and_then(|b| u32::try_from(b).ok())
        else {
            return;
        };
        let buffer_desc = D3D11_BUFFER_DESC {
            Usage: D3D11_USAGE_DYNAMIC,
            ByteWidth: byte_width,
            BindFlags: bind_flags.0 as u32,
            CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
            MiscFlags: 0,
            StructureByteStride: 0
        };
        
        let sub_data = D3D11_SUBRESOURCE_DATA {
            pSysMem: data,
            SysMemPitch: 0,
            SysMemSlicePitch: 0
        };
        let _ = self.create_buffer_or_update(d3d11_cx, &buffer_desc, &sub_data, len_slots, data);
    }
    
    pub fn update_with_u32_index_data(&mut self, d3d11_cx: &D3d11Cx, data: &[u32]) {
        self.update_with_data(d3d11_cx, D3D11_BIND_INDEX_BUFFER, data.len(), data.as_ptr() as *const _);
    }
    
    pub fn update_with_f32_vertex_data(&mut self, d3d11_cx: &D3d11Cx, data: &[f32]) {
        self.update_with_data(d3d11_cx, D3D11_BIND_VERTEX_BUFFER, data.len(), data.as_ptr() as *const _);
    }
    
    pub fn update_with_f32_constant_data(&mut self, d3d11_cx: &D3d11Cx, data: &[f32]) {
        if data.is_empty() {
            return;
        }
        let pad = (4 - (data.len() & 3)) & 3;
        let Some(len_slots) = data.len().checked_add(pad) else {
            return;
        };
        let Some(byte_width) = len_slots
            .checked_mul(4)
            .and_then(|b| u32::try_from(b).ok())
        else {
            return;
        };

        // Skip redundant Map(D3D11_MAP_WRITE_DISCARD) when constant buffer bytes are bit-identical
        // to the previous upload. Eliminates ~95% of per-frame Intel dynamic-buffer renamings.
        if self.buffer.is_some()
            && self.last_size == len_slots
            && self.last_cb_u32.len() == len_slots
            && self.last_cb_u32[data.len()..].iter().all(|&p| p == 0)
        {
            let mut unchanged = true;
            for (i, &v) in data.iter().enumerate() {
                if self.last_cb_u32[i] != v.to_bits() {
                    unchanged = false;
                    break;
                }
            }
            if unchanged {
                return;
            }
        }

        self.last_cb_u32.clear();
        self.last_cb_u32.reserve(len_slots);
        for &v in data {
            self.last_cb_u32.push(v.to_bits());
        }
        for _ in 0..pad {
            self.last_cb_u32.push(0);
        }

        let ptr = self.last_cb_u32.as_ptr() as *const std::ffi::c_void;
        let sub_data = D3D11_SUBRESOURCE_DATA {
            pSysMem: ptr,
            SysMemPitch: 0,
            SysMemSlicePitch: 0
        };
        let buffer_desc = D3D11_BUFFER_DESC {
            Usage: D3D11_USAGE_DYNAMIC,
            ByteWidth: byte_width,
            BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
            CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
            MiscFlags: 0,
            StructureByteStride: 0
        };
        if !self.create_buffer_or_update(d3d11_cx, &buffer_desc, &sub_data, len_slots, ptr) {
            self.last_cb_u32.clear();
        }
    }
}


#[derive(Default)]
pub struct CxOsTexture {
    texture: Option<ID3D11Texture2D >,
    pub shared_handle: HANDLE,
    shader_resource_view: Option<ID3D11ShaderResourceView >,
    render_target_view: Option<ID3D11RenderTargetView >,
    depth_stencil_view: Option<ID3D11DepthStencilView >,
}

impl CxTexture {
    
    pub fn update_vec_texture(
        &mut self,
        d3d11_cx: &D3d11Cx,
    ) {
        self.previous_platform_resource = None;
        // TODO maybe we can update the data instead of making a new texture?
        if self.alloc_vec(){}
        if !self.take_updated().is_empty() {
            fn get_descs(format: DXGI_FORMAT, width: usize, height: usize, bpp: usize, data: *const std::ffi::c_void)->(D3D11_SUBRESOURCE_DATA,D3D11_TEXTURE2D_DESC) {
                let sub_data = D3D11_SUBRESOURCE_DATA {
                    pSysMem: data,
                    SysMemPitch: (width * bpp) as u32,
                    SysMemSlicePitch: 0
                };
                                            
                let texture_desc = D3D11_TEXTURE2D_DESC {
                    Width: width as u32,
                    Height: height as u32,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: format,
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: 1,
                        Quality: 0
                    },
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
                    CPUAccessFlags: 0,
                    MiscFlags: 0,
                };
                (sub_data,texture_desc)
            }
            
            let release_cpu = self.release_cpu_after_upload;
            let (sub_data, mut texture_desc, is_immutable_cover) = match &self.format{
                TextureFormat::VecBGRAu8_32{width, height, data, ..}=>{
                    let Some(buf) = data.as_ref() else { return; };
                    let (sd, td) = get_descs(DXGI_FORMAT_B8G8R8A8_UNORM, *width, *height, 4, buf.as_ptr() as *const _);
                    (sd, td, release_cpu)
                }
                TextureFormat::VecRGBAf32{width, height, data, ..}=>{
                    let Some(buf) = data.as_ref() else { return; };
                    let (sd, td) = get_descs(DXGI_FORMAT_R32G32B32A32_FLOAT, *width, *height, 16, buf.as_ptr() as *const _);
                    (sd, td, false)
                }
                TextureFormat::VecRu8{width, height, data, ..}=>{
                    let Some(buf) = data.as_ref() else { return; };
                    let (sd, td) = get_descs(DXGI_FORMAT_R8_UNORM, *width, *height, 1, buf.as_ptr() as *const _);
                    (sd, td, false)
                }
                TextureFormat::VecRGu8{width, height, data, ..}=>{
                    let Some(buf) = data.as_ref() else { return; };
                    let (sd, td) = get_descs(DXGI_FORMAT_R8G8_UNORM, *width, *height, 1, buf.as_ptr() as *const _);
                    (sd, td, false)
                }
                TextureFormat::VecRf32{width, height, data, ..}=>{
                    let Some(buf) = data.as_ref() else { return; };
                    let (sd, td) = get_descs(DXGI_FORMAT_R32_FLOAT, *width, *height, 4, buf.as_ptr() as *const _);
                    (sd, td, false)
                }
                _=>return
            };
            if is_immutable_cover {
                // Static cover artwork never mutates on GPU: IMMUTABLE eliminates driver staging/shadow copies.
                texture_desc.Usage = D3D11_USAGE_IMMUTABLE;
            }
            let mut texture = None;
            let mut shader_resource_view = None;
            if unsafe { d3d11_cx.device.CreateTexture2D(&texture_desc, Some(&sub_data), Some(&mut texture)) }.is_err() {
                return;
            }
            let Some(tex_ref) = texture.as_ref() else {
                return;
            };
            let Ok(resource): crate::windows::core::Result<ID3D11Resource> = tex_ref.cast() else {
                return;
            };
            if unsafe { d3d11_cx.device.CreateShaderResourceView(&resource, None, Some(&mut shader_resource_view)) }.is_err() {
                return;
            }
            self.os.texture = texture;
            self.os.shader_resource_view = shader_resource_view;
            // Release CPU backing Vec<u32> ONLY for write-once cover textures that explicitly opted in
            // (never for mutable Makepad font/color glyph atlases that later call take_vec_u32).
            if is_immutable_cover {
                if let TextureFormat::VecBGRAu8_32 { data, .. } = &mut self.format {
                    *data = None;
                }
            }
        }
    }
    
    pub fn update_render_target(
        &mut self,
        d3d11_cx: &D3d11Cx,
        width: usize,
        height: usize
    ) {
        self.previous_platform_resource = None;
        if self.alloc_render(width, height){
            let alloc = self.alloc.as_ref().unwrap();
            let misc_flags = D3D11_RESOURCE_MISC_FLAG(0);
            let format = texture_pixel_to_dx11_pixel(&alloc.pixel);
            
            let texture_desc = D3D11_TEXTURE2D_DESC {
                Width: width as u32,
                Height: height as u32,
                MipLevels: 1,
                ArraySize: 1,
                Format: format,
                SampleDesc: DXGI_SAMPLE_DESC {Count: 1, Quality: 0},
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
                CPUAccessFlags: 0,
                MiscFlags: misc_flags.0 as u32,
            };
            
            let mut texture = None;
            unsafe {d3d11_cx.device.CreateTexture2D(&texture_desc, None, Some(&mut texture)).unwrap()};
            let resource: ID3D11Resource = texture.clone().unwrap().cast().unwrap();
            let mut shader_resource_view = None;
            unsafe {d3d11_cx.device.CreateShaderResourceView(&resource, None, Some(&mut shader_resource_view)).unwrap()};
            let mut render_target_view = None;
            unsafe {d3d11_cx.device.CreateRenderTargetView(&resource, None, Some(&mut render_target_view)).unwrap()};
            
            self.os.texture = texture;
            self.os.shader_resource_view = shader_resource_view;
            self.os.render_target_view = render_target_view;
        }
    }
    
    pub fn update_depth_stencil(
        &mut self,
        d3d11_cx: &D3d11Cx,
        width: usize,
        height: usize
    ) {
        self.previous_platform_resource = None;
        if self.alloc_depth(width, height){
            let alloc = self.alloc.as_ref().unwrap();
            let format;
            match alloc.pixel {
                TexturePixel::D32 => {
                    format = DXGI_FORMAT_D32_FLOAT;
                }
                _ => {
                    panic!("Wrong format for update_depth_stencil");
                }
            }
            let texture_desc = D3D11_TEXTURE2D_DESC {
                Width: width as u32,
                Height: height as u32,
                MipLevels: 1,
                ArraySize: 1,
                Format: format,
                SampleDesc: DXGI_SAMPLE_DESC {Count: 1, Quality: 0},
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: D3D11_BIND_DEPTH_STENCIL.0 as u32, // | D3D11_BIND_SHADER_RESOURCE,
                CPUAccessFlags: 0,
                MiscFlags: 0,
            };
            
            let mut texture = None;
            unsafe {d3d11_cx.device.CreateTexture2D(&texture_desc, None, Some(&mut texture)).unwrap()};
            let resource: ID3D11Resource = texture.clone().unwrap().cast().unwrap();
            //let shader_resource_view = unsafe {d3d11_cx.device.CreateShaderResourceView(&texture, None).unwrap()};
            
            let dsv_desc = D3D11_DEPTH_STENCIL_VIEW_DESC {
                Format: DXGI_FORMAT_D32_FLOAT,
                ViewDimension: D3D11_DSV_DIMENSION_TEXTURE2D,
                Flags: 0,
                ..Default::default()
            };
            
            let mut depth_stencil_view = None;
            unsafe {d3d11_cx.device.CreateDepthStencilView(&resource, Some(&dsv_desc), Some(&mut depth_stencil_view)).unwrap()};
            
            self.os.depth_stencil_view = depth_stencil_view;
            self.os.texture = texture;
            self.os.shader_resource_view = None; //Some(shader_resource_view);
        }
    }
    
    fn update_shared_texture(
        &mut self,
        d3d11_device: &ID3D11Device,
    ) {
        self.previous_platform_resource = None;
        if self.alloc_shared(){
            let alloc = self.alloc.as_ref().unwrap();

            let texture_desc = D3D11_TEXTURE2D_DESC {
                Width: alloc.width as u32,
                Height: alloc.height as u32,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0
                },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
                CPUAccessFlags: 0,
                MiscFlags: 2,  // D3D11_RESOURCE_MISC_SHARED
            };
            
            let mut texture = None;
            unsafe {d3d11_device.CreateTexture2D(&texture_desc, None, Some(&mut texture)).unwrap()};
            let resource: ID3D11Resource = texture.clone().unwrap().cast().unwrap();
            let mut shader_resource_view = None;
            unsafe {d3d11_device.CreateShaderResourceView(&resource, None, Some(&mut shader_resource_view)).unwrap()};

            // get IDXGIResource interface on newly created texture object
            let dxgi_resource: IDXGIResource = resource.cast().unwrap();
            //let mut dxgi_resource_ptr = None;
            //unsafe { resource.query(IDXGIResource::IID,Some(&mut dxgi_resource_ptr)).unwrap() };
            //let dxgi_resource = dxgi_resource_ptr.as_ref().unwrap().into();

            // get shared handle of this resource
            let handle = unsafe { dxgi_resource.GetSharedHandle().unwrap() };
            //log!("created new shared texture with handle {:?}",handle);

            self.os.texture = texture;
            self.os.shader_resource_view = shader_resource_view;
            self.os.shared_handle = handle;
        }
    }

    pub fn update_from_shared_handle(&mut self,
        d3d11_cx: &D3d11Cx,
        handle: HANDLE,
    ) {
        if self.alloc_shared(){
            let mut texture: Option<ID3D11Texture2D> = None;
            if let Ok(()) = unsafe { d3d11_cx.device.OpenSharedResource(handle,&mut texture) } {
    
                let resource: ID3D11Resource = texture.clone().unwrap().cast().unwrap();
                let mut shader_resource_view = None;
                unsafe {d3d11_cx.device.CreateShaderResourceView(&resource, None, Some(&mut shader_resource_view)).unwrap()};
                let mut render_target_view = None;
                unsafe {d3d11_cx.device.CreateRenderTargetView(&resource, None, Some(&mut render_target_view)).unwrap()};
                self.os.texture = texture;
                self.os.render_target_view = render_target_view;
                self.os.shader_resource_view = shader_resource_view;
            }
        }
    }
}

impl CxOsPass {
    pub fn set_states(&mut self, d3d11_cx: &D3d11Cx,) {
        
        if self.blend_state.is_none() {
            
            let mut blend_desc: D3D11_BLEND_DESC = Default::default();
            blend_desc.AlphaToCoverageEnable = FALSE;
            blend_desc.RenderTarget[0] = D3D11_RENDER_TARGET_BLEND_DESC {
                BlendEnable: TRUE,
                SrcBlend: D3D11_BLEND_ONE,
                SrcBlendAlpha: D3D11_BLEND_ONE,
                DestBlend: D3D11_BLEND_INV_SRC_ALPHA,
                DestBlendAlpha: D3D11_BLEND_INV_SRC_ALPHA,
                BlendOp: D3D11_BLEND_OP_ADD,
                BlendOpAlpha: D3D11_BLEND_OP_ADD,
                RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
            };
            unsafe {d3d11_cx.device.CreateBlendState(&blend_desc, Some(&mut self.blend_state)).unwrap()}
        }
        
        if self.raster_state.is_none() {
            let raster_desc = D3D11_RASTERIZER_DESC {
                AntialiasedLineEnable: FALSE,
                CullMode: D3D11_CULL_NONE,
                DepthBias: 0,
                DepthBiasClamp: 0.0,
                DepthClipEnable: TRUE,
                FillMode: D3D11_FILL_SOLID,
                FrontCounterClockwise: FALSE,
                MultisampleEnable: FALSE,
                ScissorEnable: FALSE,
                SlopeScaledDepthBias: 0.0,
            };
            unsafe {d3d11_cx.device.CreateRasterizerState(&raster_desc, Some(&mut self.raster_state)).unwrap()}
        }
        
        if self.depth_stencil_state.is_none() {
            let ds_desc = D3D11_DEPTH_STENCIL_DESC {
                DepthEnable: TRUE,
                DepthWriteMask: D3D11_DEPTH_WRITE_MASK_ALL,
                DepthFunc: D3D11_COMPARISON_LESS_EQUAL,
                StencilEnable: FALSE,
                StencilReadMask: 0xff,
                StencilWriteMask: 0xff,
                FrontFace: D3D11_DEPTH_STENCILOP_DESC {
                    StencilFailOp: D3D11_STENCIL_OP_REPLACE,
                    StencilDepthFailOp: D3D11_STENCIL_OP_REPLACE,
                    StencilPassOp: D3D11_STENCIL_OP_REPLACE,
                    StencilFunc: D3D11_COMPARISON_ALWAYS,
                },
                BackFace: D3D11_DEPTH_STENCILOP_DESC {
                    StencilFailOp: D3D11_STENCIL_OP_REPLACE,
                    StencilDepthFailOp: D3D11_STENCIL_OP_REPLACE,
                    StencilPassOp: D3D11_STENCIL_OP_REPLACE,
                    StencilFunc: D3D11_COMPARISON_ALWAYS,
                },
            };
            unsafe {d3d11_cx.device.CreateDepthStencilState(&ds_desc, Some(&mut self.depth_stencil_state)).unwrap()}
        }
        
        if self.sampler_state.is_none() {
            let samp_desc = D3D11_SAMPLER_DESC {
                Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
                AddressU: D3D11_TEXTURE_ADDRESS_WRAP,
                AddressV: D3D11_TEXTURE_ADDRESS_WRAP,
                AddressW: D3D11_TEXTURE_ADDRESS_WRAP,
                MipLODBias: 0.0,
                MaxAnisotropy: 1,
                ComparisonFunc: D3D11_COMPARISON_NEVER,
                BorderColor: [0.0, 0.0, 0.0, 0.0],
                MinLOD: 0.0,
                MaxLOD: f32::MAX,
            };
            unsafe {
                let _ = d3d11_cx.device.CreateSamplerState(&samp_desc, Some(&mut self.sampler_state));
            }
        }
        
        unsafe {
            d3d11_cx.context.RSSetState(self.raster_state.as_ref().unwrap());
            let blend_factor = [0., 0., 0., 0.];
            d3d11_cx.context.OMSetBlendState(self.blend_state.as_ref().unwrap(), Some(&blend_factor), 0xffffffff);
            d3d11_cx.context.OMSetDepthStencilState(self.depth_stencil_state.as_ref().unwrap(), 0);
            if let Some(sampler) = &self.sampler_state {
                let samplers = [Some(sampler.clone())];
                d3d11_cx.context.PSSetSamplers(0, Some(&samplers));
            }
        }
    }
}

#[derive(Default, Clone)]
pub struct CxOsPass {
    pass_uniforms: D3d11Buffer,
    blend_state: Option<ID3D11BlendState >,
    raster_state: Option<ID3D11RasterizerState >,
    depth_stencil_state: Option<ID3D11DepthStencilState >,
    sampler_state: Option<ID3D11SamplerState >,
}

#[derive(Default, Clone)]
pub struct CxOsGeometry {
    pub geom_vbuf: D3d11Buffer,
    pub geom_ibuf: D3d11Buffer,
}

#[derive(Clone)]
pub struct CxOsDrawShader {
    pub hlsl: String,
    pub const_table_uniforms: D3d11Buffer,
    pub live_uniforms: D3d11Buffer,
    pub pixel_shader: ID3D11PixelShader,
    pub vertex_shader: ID3D11VertexShader,
    pub input_layout: ID3D11InputLayout
}

impl CxOsDrawShader {
    
    fn new(d3d11_cx: &D3d11Cx, hlsl: String, mapping: &CxDrawShaderMapping) -> Option<Self> {
        
        fn compile_shader(target: &str, entry: &str, shader: &str) -> Result<ID3DBlob, String> {
            unsafe {
                let shader_bytes = shader.as_bytes();
                let mut blob = None;
                let mut errors = None;
                if D3DCompile(
                    shader_bytes.as_ptr() as *const _,
                    shader_bytes.len(),
                    PCSTR("makepad_shader\0".as_ptr()), // sourcename
                    None, // defines
                    None, // include
                    PCSTR(entry.as_ptr()), // entry point
                    PCSTR(target.as_ptr()), // target
                    0, // flags1
                    0, // flags2
                    &mut blob,
                    Some(&mut errors)
                ).is_ok() {
                    return Ok(blob.unwrap());
                };
                let error = errors.unwrap();
                let pointer = error.GetBufferPointer();
                let size = error.GetBufferSize();
                let slice = std::slice::from_raw_parts(pointer as *const u8, size as usize);
                return Err(String::from_utf8_lossy(slice).into_owned());
            }
        }
        fn split_source(src: &str) -> String {
            let mut r = String::new();
            let split = src.split("\n");
            for (line, chunk) in split.enumerate() {
                r.push_str(&(line + 1).to_string());
                r.push_str(":");
                r.push_str(chunk);
                r.push_str("\n");
            }
            return r
        }
        
        fn slots_to_dxgi_format(slots: usize) -> DXGI_FORMAT {
            
            match slots {
                1 => DXGI_FORMAT_R32_FLOAT,
                2 => DXGI_FORMAT_R32G32_FLOAT,
                3 => DXGI_FORMAT_R32G32B32_FLOAT,
                4 => DXGI_FORMAT_R32G32B32A32_FLOAT,
                _ => panic!("slots_to_dxgi_format unsupported slotcount {}", slots)
            }
        }
        
        let vs_blob = match compile_shader("vs_5_0\0", "vertex_main\0", &hlsl) {
            Err(msg) => {
                println!("Cannot compile vertexshader\n{}\n{}", msg, split_source(&hlsl));
                return None
            },
            Ok(blob) => {
                blob
            }
        };
        
        let ps_blob = match compile_shader("ps_5_0\0", "pixel_main\0", &hlsl) {
            Err(msg) => {
                println!("Cannot compile pixelshader\n{}\n{}", msg, split_source(&hlsl));
                return None
            },
            Ok(blob) => {
                blob
            }
        };
        
        let mut vs = None;
        unsafe {d3d11_cx.device.CreateVertexShader(
            std::slice::from_raw_parts(vs_blob.GetBufferPointer() as *const u8, vs_blob.GetBufferSize() as usize),
            None,
            Some(&mut vs)
        ).unwrap()};
        
        let mut ps = None;
        unsafe {d3d11_cx.device.CreatePixelShader(
            std::slice::from_raw_parts(ps_blob.GetBufferPointer() as *const u8, ps_blob.GetBufferSize() as usize),
            None,
            Some(&mut ps)
        ).unwrap()};
        
        let mut layout_desc = Vec::new();
        let mut strings = Vec::new();
        
        for (index, geom) in mapping.geometries.inputs.iter().enumerate() {
            
            strings.push(format!("GEOM{}\0", generate_hlsl::index_to_char(index))); //std::char::from_u32(index as u32 + 65).unwrap())).unwrap());
            
            layout_desc.push(D3D11_INPUT_ELEMENT_DESC {
                SemanticName: PCSTR(strings.last().unwrap().as_ptr()),
                SemanticIndex: 0,
                Format: slots_to_dxgi_format(geom.slots),
                InputSlot: 0,
                AlignedByteOffset: (geom.offset * 4) as u32,
                InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
                InstanceDataStepRate: 0
            })
        }
        
        let mut index = 0;
        for inst in &mapping.instances.inputs {
            if inst.slots == 16 {
                for i in 0..4 {
                    strings.push(format!("INST{}\0", generate_hlsl::index_to_char(index))); //std::char::from_u32(index as u32 + 65).unwrap())).unwrap());
                    layout_desc.push(D3D11_INPUT_ELEMENT_DESC {
                        SemanticName: PCSTR(strings.last().unwrap().as_ptr()),
                        SemanticIndex: 0,
                        Format: slots_to_dxgi_format(4),
                        InputSlot: 1,
                        AlignedByteOffset: (inst.offset * 4 + i * 16) as u32,
                        InputSlotClass: D3D11_INPUT_PER_INSTANCE_DATA,
                        InstanceDataStepRate: 1
                    });
                    index += 1;
                }
            }
            else if inst.slots == 9 {
                for i in 0..3 {
                    strings.push(format!("INST{}\0", generate_hlsl::index_to_char(index))); //std::char::from_u32(index as u32 + 65).unwrap())).unwrap());
                    layout_desc.push(D3D11_INPUT_ELEMENT_DESC {
                        SemanticName: PCSTR(strings.last().unwrap().as_ptr()),
                        SemanticIndex: 0,
                        Format: slots_to_dxgi_format(3),
                        InputSlot: 1,
                        AlignedByteOffset: (inst.offset * 4 + i * 9) as u32,
                        InputSlotClass: D3D11_INPUT_PER_INSTANCE_DATA,
                        InstanceDataStepRate: 1
                    });
                    index += 1;
                }
            }
            else {
                strings.push(format!("INST{}\0", generate_hlsl::index_to_char(index))); //std::char::from_u32(index as u32 + 65).unwrap())).unwrap());
                layout_desc.push(D3D11_INPUT_ELEMENT_DESC {
                    SemanticName: PCSTR(strings.last().unwrap().as_ptr()),
                    SemanticIndex: 0,
                    Format: slots_to_dxgi_format(inst.slots),
                    InputSlot: 1,
                    AlignedByteOffset: (inst.offset * 4) as u32,
                    InputSlotClass: D3D11_INPUT_PER_INSTANCE_DATA,
                    InstanceDataStepRate: 1
                });
                index += 1;
            }
        }
        
        let mut input_layout = None;
        unsafe {
            d3d11_cx.device.CreateInputLayout(
                &layout_desc,
                std::slice::from_raw_parts(vs_blob.GetBufferPointer() as *const u8, vs_blob.GetBufferSize() as usize),
                Some(&mut input_layout)
            ).unwrap()
        };
        
        let mut live_uniforms = D3d11Buffer::default();
        live_uniforms.update_with_f32_constant_data(d3d11_cx, mapping.live_uniforms_buf.as_ref());
        
        let mut const_table_uniforms = D3d11Buffer::default();
        const_table_uniforms.update_with_f32_constant_data(d3d11_cx, mapping.const_table.table.as_ref());
        
        // Drop compiled DXBC blobs right after creating shader & input layout to free startup RAM.
        drop(ps_blob);
        drop(vs_blob);
        Some(Self {
            hlsl,
            const_table_uniforms,
            live_uniforms,
            pixel_shader: ps.unwrap(),
            vertex_shader: vs.unwrap(),
            input_layout: input_layout.unwrap()
        })
    }
}
