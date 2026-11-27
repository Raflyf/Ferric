use {
    std::{
        fmt,
        fs::File,
        io::prelude::*,
        ops::Deref,
        path::PathBuf,
        rc::Rc,
        sync::Arc,
        time::SystemTime,
    },
    crate::{
        cx::Cx,
    },
};

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum EventFlow{
    Poll,
    Wait,
    Exit
}

fn is_valid_sfnt_or_ttc(buf: &[u8]) -> bool {
    if buf.len() < 12 || buf.len() > 32 * 1024 * 1024 {
        return false;
    }
    matches!(
        &buf[0..4],
        [0x00, 0x01, 0x00, 0x00] | b"OTTO" | b"true" | b"ttcf"
    )
}

#[cfg(target_os = "windows")]
pub struct MappedFontBytes {
    ptr: *const u8,
    len: usize,
    mapping: crate::windows::Win32::Foundation::HANDLE,
}

#[cfg(target_os = "windows")]
unsafe impl Send for MappedFontBytes {}
#[cfg(target_os = "windows")]
unsafe impl Sync for MappedFontBytes {}

#[cfg(target_os = "windows")]
impl MappedFontBytes {
    fn map_file(file: &File) -> Option<Self> {
        use std::os::windows::io::AsRawHandle;
        use crate::windows::Win32::{
            Foundation::{CloseHandle, HANDLE},
            System::Memory::{
                CreateFileMappingW, MapViewOfFile, UnmapViewOfFile, FILE_MAP_READ,
                MEMORY_MAPPED_VIEW_ADDRESS, PAGE_READONLY,
            },
        };
        let len_u64 = file.metadata().ok()?.len();
        if len_u64 < 12 || len_u64 > 32 * 1024 * 1024 {
            return None;
        }
        let len = usize::try_from(len_u64).ok()?;
        let file_handle = HANDLE(file.as_raw_handle() as isize);
        unsafe {
            let mapping = CreateFileMappingW(file_handle, None, PAGE_READONLY, 0, 0, None).ok()?;
            if mapping.is_invalid() {
                return None;
            }
            let view: MEMORY_MAPPED_VIEW_ADDRESS = MapViewOfFile(mapping, FILE_MAP_READ, 0, 0, len);
            if view.Value.is_null() {
                let _ = CloseHandle(mapping);
                return None;
            }
            let ptr = view.Value as *const u8;
            let slice = std::slice::from_raw_parts(ptr, len);
            if !is_valid_sfnt_or_ttc(slice) {
                let _ = UnmapViewOfFile(view);
                let _ = CloseHandle(mapping);
                return None;
            }
            Some(Self { ptr, len, mapping })
        }
    }
}

#[cfg(target_os = "windows")]
impl Deref for MappedFontBytes {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }
}

#[cfg(target_os = "windows")]
impl Drop for MappedFontBytes {
    fn drop(&mut self) {
        use crate::windows::Win32::{
            Foundation::CloseHandle,
            System::Memory::{UnmapViewOfFile, MEMORY_MAPPED_VIEW_ADDRESS},
        };
        unsafe {
            let _ = UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                Value: self.ptr as *mut std::ffi::c_void,
            });
            let _ = CloseHandle(self.mapping);
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub struct MappedFontBytes(Vec<u8>);

#[cfg(not(target_os = "windows"))]
impl MappedFontBytes {
    fn map_file(file: &File) -> Option<Self> {
        let len_u64 = file.metadata().ok()?.len();
        if len_u64 < 12 || len_u64 > 32 * 1024 * 1024 {
            return None;
        }
        let mut f = file.try_clone().ok()?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).ok()?;
        if !is_valid_sfnt_or_ttc(&buf) {
            return None;
        }
        Some(Self(buf))
    }
}

#[cfg(not(target_os = "windows"))]
impl Deref for MappedFontBytes {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for MappedFontBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MappedFontBytes")
            .field("len", &self.len())
            .finish()
    }
}

/// Shared font backing bytes: either heap-owned (`Rc<Vec<u8>>`) or read-only OS file-mapped
/// (`Arc<MappedFontBytes>`, zero Private Working Set / zero Private Commit on Windows).
#[derive(Clone)]
pub enum FontData {
    Owned(Rc<Vec<u8>>),
    Mapped(Arc<MappedFontBytes>),
}

impl Deref for FontData {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            Self::Owned(v) => v.as_slice(),
            Self::Mapped(m) => m.deref(),
        }
    }
}

impl From<Rc<Vec<u8>>> for FontData {
    fn from(v: Rc<Vec<u8>>) -> Self {
        Self::Owned(v)
    }
}

impl fmt::Debug for FontData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Owned(v) => f.debug_tuple("FontData::Owned").field(&v.len()).finish(),
            Self::Mapped(m) => f.debug_tuple("FontData::Mapped").field(&m.len()).finish(),
        }
    }
}

pub fn try_map_system_fallback_font(virtual_path: &str) -> Option<FontData> {
    let file_name = virtual_path.rsplit(['/', '\\']).next()?;
    let candidates: Vec<PathBuf> = {
        #[cfg(target_os = "windows")]
        {
            let sys_root = std::env::var_os("SystemRoot")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
            let fonts_dir = sys_root.join("Fonts");
            let names: &[&str] = match file_name {
                "__sys_cjk_jp.ttf" => &[
                    "YuGothM.ttc",
                    "YuGothR.ttc",
                    "YuGothB.ttc",
                    "meiryo.ttc",
                    "msgothic.ttc",
                    "msyh.ttc",
                ],
                "__sys_cjk_kr.ttf" => &["malgunsl.ttf", "malgun.ttf", "gulim.ttc"],
                "__sys_cjk_cn.ttf" => &["msyhl.ttc", "msyh.ttc", "simsun.ttc"],
                "__sys_thai.ttf" => &["LeelawUI.ttf", "leelawad.ttf", "tahoma.ttf", "segoeui.ttf"],
                _ => return None,
            };
            names.iter().map(|n| fonts_dir.join(n)).collect()
        }
        #[cfg(target_os = "macos")]
        {
            let paths: &[&str] = match file_name {
                "__sys_cjk_jp.ttf" => &[
                    "/System/Library/Fonts/Hiragino Sans GB.ttc",
                    "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
                ],
                "__sys_cjk_kr.ttf" => &["/System/Library/Fonts/AppleSDGothicNeo.ttc"],
                "__sys_cjk_cn.ttf" => &["/System/Library/Fonts/PingFang.ttc"],
                "__sys_thai.ttf" => &["/System/Library/Fonts/Thonburi.ttc"],
                _ => return None,
            };
            paths.iter().map(PathBuf::from).collect()
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            let paths: &[&str] = match file_name {
                "__sys_cjk_jp.ttf" => &[
                    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
                    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
                ],
                "__sys_cjk_kr.ttf" => &[
                    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
                    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
                ],
                "__sys_cjk_cn.ttf" => &[
                    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
                    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
                ],
                "__sys_thai.ttf" => &[
                    "/usr/share/fonts/truetype/noto/NotoSansThai-Regular.ttf",
                ],
                _ => return None,
            };
            paths.iter().map(PathBuf::from).collect()
        }
        #[cfg(not(any(target_os = "windows", unix)))]
        {
            let _ = file_name;
            Vec::new()
        }
    };

    for cand in candidates {
        if let Ok(f) = File::open(&cand) {
            if let Some(mapped) = MappedFontBytes::map_file(&f) {
                return Some(FontData::Mapped(Arc::new(mapped)));
            }
        }
    }
    None
}

pub fn is_virtual_system_font(path: &str) -> bool {
    matches!(
        path.rsplit(['/', '\\']).next(),
        Some("__sys_cjk_jp.ttf" | "__sys_cjk_kr.ttf" | "__sys_cjk_cn.ttf" | "__sys_thai.ttf")
    )
}

// lets start a websocket thread


impl Cx {
    
    pub fn native_load_dependencies(&mut self){
        let empty_virtual = Rc::new(Vec::new());

        // Pass 1: Load real bundled disk resources (Inter TTFs, small icons).
        // Pass 2: Virtual OS CJK/Thai fallback fonts are NOT read into heap at startup — they are
        // mapped read-only (`MapViewOfFile`, 0 MB Private Commit/WS) lazily by `makepad-draw` only
        // when a non-Latin glyph actually requires fallback shaping.
        for (path, dep) in &mut self.dependencies {
            if is_virtual_system_font(path) {
                dep.data = Some(Ok(empty_virtual.clone()));
                continue;
            }
            if let Ok(mut file_handle) = File::open(path) {
                let mut buffer = Vec::<u8>::new();
                if file_handle.read_to_end(&mut buffer).is_ok() {
                    dep.data = Some(Ok(Rc::new(buffer)));
                }
                else{
                    dep.data = Some(Err("read_to_end failed".to_string()));
                }
            }
            else{
                println!("Could not load resource {}", path);
                dep.data = Some(Err("File! open failed".to_string()));
            }
        }
    }
    
    pub fn time_now()->f64{
        if let Ok(elapsed) = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH){
            return elapsed.as_secs_f64();
        }
        return 0.0
    }
}
