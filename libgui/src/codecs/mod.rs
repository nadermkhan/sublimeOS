use alloc::vec::Vec;

pub mod audio;
pub mod avi;
pub mod deflate;
pub mod jpeg;
pub mod mp3;
pub mod png;
pub mod qoi;
pub mod raster;

pub use audio::{
    decode_audio, decode_qoa, decode_wav, encode_ima_adpcm_mono, encode_qoa_mono, inspect_audio,
    inspect_audio_metadata, resample_to_44k_stereo, AudioMetadata, DecodedAudio,
};
pub use avi::{build_mjpeg_avi, decode_avi, inspect_avi, AviMovie};
pub use jpeg::{decode_jpeg, encode_baseline_jpeg, inspect_jpeg};
pub use png::{decode_png, encode_png_argb, inspect_png};
pub use qoi::{decode_qoi, encode_qoi_argb, inspect_qoi};

#[derive(Clone)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u32>,
    pub format_name: &'static str,
}

/// Inspect image header dimensions (`(width, height, format_label)`) across PNG, JPEG, QOI, BMP, TGA, PPM.
pub fn inspect_image(data: &[u8]) -> Option<(u32, u32, &'static str)> {
    png::inspect_png(data)
        .or_else(|| jpeg::inspect_jpeg(data))
        .or_else(|| qoi::inspect_qoi(data))
        .or_else(|| raster::inspect_raster(data))
}

/// Decode any supported image file (`PNG`, `JPEG`, `QOI`, `BMP`, `TGA`, `PPM`) into ARGB8888 pixels.
pub fn decode_image(data: &[u8]) -> Result<DecodedImage, &'static str> {
    if png::inspect_png(data).is_some() {
        return png::decode_png(data);
    }
    if jpeg::inspect_jpeg(data).is_some() {
        return jpeg::decode_jpeg(data);
    }
    if qoi::inspect_qoi(data).is_some() {
        return qoi::decode_qoi(data);
    }
    if data.len() >= 54 && &data[0..2] == b"BM" {
        return raster::decode_bmp(data);
    }
    if data.len() >= 7 && (data.starts_with(b"P6") || data.starts_with(b"P3")) {
        return raster::decode_ppm(data);
    }
    if data.len() >= 18 {
        let img_type = data[2];
        let bpp = data[16];
        if (img_type == 2 || img_type == 10) && (bpp == 24 || bpp == 32) {
            return raster::decode_tga(data);
        }
    }
    Err("Unsupported image format (use PNG, JPEG, QOI, BMP, TGA, or PPM)")
}

fn ends_with_ci(path: &str, ext: &str) -> bool {
    let pb = path.as_bytes();
    let eb = ext.as_bytes();
    if pb.len() < eb.len() {
        return false;
    }
    let tail = &pb[pb.len() - eb.len()..];
    tail.iter().zip(eb.iter()).all(|(&a, &b)| a.to_ascii_lowercase() == b.to_ascii_lowercase())
}

pub fn is_image_path(path: &str) -> bool {
    ends_with_ci(path, ".png")
        || ends_with_ci(path, ".jpg")
        || ends_with_ci(path, ".jpeg")
        || ends_with_ci(path, ".qoi")
        || ends_with_ci(path, ".bmp")
        || ends_with_ci(path, ".tga")
        || ends_with_ci(path, ".ppm")
}

pub fn is_audio_path(path: &str) -> bool {
    ends_with_ci(path, ".wav")
        || ends_with_ci(path, ".mp3")
        || ends_with_ci(path, ".qoa")
}

pub fn is_video_path(path: &str) -> bool {
    ends_with_ci(path, ".avi")
}

pub fn is_media_path(path: &str) -> bool {
    is_audio_path(path) || is_video_path(path) || is_image_path(path)
}

pub fn is_code_path(path: &str) -> bool {
    ends_with_ci(path, ".rs")
        || ends_with_ci(path, ".c")
        || ends_with_ci(path, ".h")
        || ends_with_ci(path, ".cpp")
        || ends_with_ci(path, ".hpp")
        || ends_with_ci(path, ".py")
        || ends_with_ci(path, ".js")
        || ends_with_ci(path, ".ts")
        || ends_with_ci(path, ".json")
        || ends_with_ci(path, ".toml")
        || ends_with_ci(path, ".ini")
        || ends_with_ci(path, ".sh")
        || ends_with_ci(path, ".css")
        || ends_with_ci(path, ".html")
        || ends_with_ci(path, ".md")
        || ends_with_ci(path, ".sublimerc")
        || ends_with_ci(path, ".zshrc")
}
