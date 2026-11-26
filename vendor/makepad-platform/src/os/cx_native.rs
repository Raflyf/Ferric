use {
    std::{
        fs::File,
        io::prelude::*,
        path::PathBuf,
        rc::Rc,
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

fn try_load_system_fallback_font(virtual_path: &str) -> Option<Rc<Vec<u8>>> {
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
                "__sys_cjk_cn.ttf" => &["msyh.ttc", "msyhl.ttc", "simsun.ttc"],
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
        if let Ok(mut f) = File::open(&cand) {
            if let Ok(meta) = f.metadata() {
                if meta.len() > 32 * 1024 * 1024 {
                    continue;
                }
            }
            let mut buf = Vec::new();
            if f.read_to_end(&mut buf).is_ok() && is_valid_sfnt_or_ttc(&buf) {
                return Some(Rc::new(buf));
            }
        }
    }
    None
}

fn is_virtual_system_font(path: &str) -> bool {
    matches!(
        path.rsplit(['/', '\\']).next(),
        Some("__sys_cjk_jp.ttf" | "__sys_cjk_kr.ttf" | "__sys_cjk_cn.ttf" | "__sys_thai.ttf")
    )
}

// lets start a websocket thread


impl Cx {
    
    pub fn native_load_dependencies(&mut self){
        let mut fallback_font: Option<Rc<Vec<u8>>> = None;

        // Pass 1: Load real bundled disk resources first and capture a valid fallback TTF.
        for (path, dep) in &mut self.dependencies {
            if is_virtual_system_font(path) {
                continue;
            }
            if let Ok(mut file_handle) = File::open(path) {
                let mut buffer = Vec::<u8>::new();
                if file_handle.read_to_end(&mut buffer).is_ok() {
                    let rc = Rc::new(buffer);
                    if (path.ends_with("Inter-Regular.ttf")
                        || (fallback_font.is_none() && path.ends_with(".ttf")))
                        && is_valid_sfnt_or_ttc(&rc)
                    {
                        fallback_font = Some(rc.clone());
                    }
                    dep.data = Some(Ok(rc));
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

        // Pass 2: Resolve virtual OS fallback font slots (Japanese, Korean, Chinese, Thai)
        // directly from system fonts with zero binary bloat, falling back safely to Inter if absent.
        for (path, dep) in &mut self.dependencies {
            if !is_virtual_system_font(path) {
                continue;
            }
            if let Some(sys_font) = try_load_system_fallback_font(path) {
                dep.data = Some(Ok(sys_font));
            } else if let Some(fb) = &fallback_font {
                dep.data = Some(Ok(fb.clone()));
            } else {
                dep.data = Some(Err("System fallback font unavailable".to_string()));
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
