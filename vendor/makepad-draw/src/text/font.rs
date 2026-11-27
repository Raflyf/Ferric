use {
    super::{
        font_face::FontFace,
        geom::{Point, Rect},
        glyph_outline,
        glyph_outline::GlyphOutline,
        glyph_raster_image::GlyphRasterImage,
        intern::Intern,
        rasterizer::{RasterizedGlyph, Rasterizer},
    },
    makepad_platform::try_map_system_fallback_font,
    makepad_rustybuzz as rustybuzz,
    rustybuzz::ttf_parser,
    std::{
        cell::{OnceCell, RefCell},
        collections::HashMap,
        hash::{Hash, Hasher},
        rc::Rc,
    },
};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct FontId(u64);

impl From<u64> for FontId {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl From<&str> for FontId {
    fn from(value: &str) -> Self {
        Self(value.intern().as_ptr() as u64)
    }
}

#[derive(Debug)]
pub struct Font {
    id: FontId,
    rasterizer: Rc<RefCell<Rasterizer>>,
    face: OnceCell<Option<FontFace>>,
    lazy_virtual_path: Option<(String, u32)>,
    ascender_fudge_in_ems: f32,
    descender_fudge_in_ems: f32,
    cached_glyph_outline_bounds_in_ems: RefCell<HashMap<GlyphId, Option<Rect<f32>>>>,
}

impl Font {
    pub fn new(
        id: FontId,
        rasterizer: Rc<RefCell<Rasterizer>>,
        face: FontFace,
        ascender_fudge_in_ems: f32,
        descender_fudge_in_ems: f32,
    ) -> Self {
        let cell = OnceCell::new();
        let _ = cell.set(Some(face));
        Self {
            id,
            rasterizer,
            face: cell,
            lazy_virtual_path: None,
            ascender_fudge_in_ems,
            descender_fudge_in_ems,
            cached_glyph_outline_bounds_in_ems: RefCell::new(HashMap::new()),
        }
    }

    pub fn new_lazy_system(
        id: FontId,
        rasterizer: Rc<RefCell<Rasterizer>>,
        virtual_path: String,
        index: u32,
        ascender_fudge_in_ems: f32,
        descender_fudge_in_ems: f32,
    ) -> Self {
        Self {
            id,
            rasterizer,
            face: OnceCell::new(),
            lazy_virtual_path: Some((virtual_path, index)),
            ascender_fudge_in_ems,
            descender_fudge_in_ems,
            cached_glyph_outline_bounds_in_ems: RefCell::new(HashMap::new()),
        }
    }

    fn face_opt(&self) -> Option<&FontFace> {
        self.face
            .get_or_init(|| {
                let (path, index) = self.lazy_virtual_path.as_ref()?;
                let data = try_map_system_fallback_font(path)?;
                FontFace::from_data_and_index(data, *index)
            })
            .as_ref()
    }

    pub fn id(&self) -> FontId {
        self.id
    }

    pub(super) fn try_ttf_parser_face(&self) -> Option<&ttf_parser::Face<'_>> {
        self.face_opt().map(|f| f.as_ttf_parser_face())
    }

    pub(super) fn try_rustybuzz_face(&self) -> Option<&rustybuzz::Face<'_>> {
        self.face_opt().map(|f| f.as_rustybuzz_face())
    }

    pub fn units_per_em(&self) -> f32 {
        self.try_ttf_parser_face()
            .map(|f| f.units_per_em() as f32)
            .unwrap_or(1000.0)
    }

    pub fn ascender_in_ems(&self) -> f32 {
        self.try_ttf_parser_face()
            .map(|f| f.ascender() as f32 / self.units_per_em())
            .unwrap_or(0.8)
            + self.ascender_fudge_in_ems
    }

    pub fn descender_in_ems(&self) -> f32 {
        self.try_ttf_parser_face()
            .map(|f| f.descender() as f32 / self.units_per_em())
            .unwrap_or(-0.2)
            + self.descender_fudge_in_ems
    }

    pub fn line_gap_in_ems(&self) -> f32 {
        self.try_ttf_parser_face()
            .map(|f| f.line_gap() as f32 / self.units_per_em())
            .unwrap_or(0.0)
    }

    pub fn glyph_outline(&self, glyph_id: GlyphId) -> Option<GlyphOutline> {
        let face = self.try_ttf_parser_face()?;
        let glyph_id = ttf_parser::GlyphId(glyph_id);
        let mut builder = glyph_outline::Builder::new();
        let bounds = face.outline_glyph(glyph_id, &mut builder)?;
        let min = Point::new(bounds.x_min as f32, bounds.y_min as f32);
        let max = Point::new(bounds.x_max as f32, bounds.y_max as f32);
        Some(builder.finish(Rect::new(min, max - min), self.units_per_em()))
    }

    pub fn glyph_outline_bounds_in_ems(
        &self,
        glyph_id: GlyphId,
        out_outline: &mut Option<GlyphOutline>,
    ) -> Option<Rect<f32>> {
        if let Some(bounds_in_ems) = self
            .cached_glyph_outline_bounds_in_ems
            .borrow()
            .get(&glyph_id)
        {
            return *bounds_in_ems;
        }
        if let Some(outline) = self.glyph_outline(glyph_id) {
            let bounds_in_ems = outline.bounds_in_ems();
            *out_outline = Some(outline);
            self.cached_glyph_outline_bounds_in_ems
                .borrow_mut()
                .insert(glyph_id, Some(bounds_in_ems));
            Some(bounds_in_ems)
        } else {
            None
        }
    }

    pub fn glyph_raster_image(
        &self,
        glyph_id: GlyphId,
        dpxs_per_em: f32,
    ) -> Option<GlyphRasterImage<'_>> {
        let face = self.try_ttf_parser_face()?;
        let glyph_id = ttf_parser::GlyphId(glyph_id);
        let image = face.glyph_raster_image(glyph_id, dpxs_per_em as u16)?;
        GlyphRasterImage::from_raster_glyph_image(image)
    }

    pub fn rasterize_glyph(&self, glyph_id: GlyphId, dpxs_per_em: f32) -> Option<RasterizedGlyph> {
        self.rasterizer
            .borrow_mut()
            .rasterize_glyph(self, glyph_id, dpxs_per_em)
    }
}

impl Eq for Font {}

impl Hash for Font {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl PartialEq for Font {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

pub type GlyphId = u16;
