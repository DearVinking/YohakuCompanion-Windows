//! 媒体封面归一化（macOS 版 CompanionMediaArtworkNormalizer 语义）：
//! 源数据 ≤16 MiB → 解码 → 长边 ≤512px 等比缩放 → PNG 重编码 → 内容 sha256。

use sha2::{Digest, Sha256};

pub const MAX_SOURCE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_THUMBNAIL_EDGE: u32 = 512;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ArtworkError {
    #[error("source exceeds {MAX_SOURCE_BYTES} bytes")]
    TooLarge,
    #[error("unsupported image: {0}")]
    Decode(String),
    #[error("encode failed: {0}")]
    Encode(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedArtwork {
    pub png: Vec<u8>,
    /// PNG 字节的小写 hex sha256（= 公网 URL 的 ?v= 值）
    pub content_hash: String,
}

pub fn normalize_artwork(bytes: &[u8]) -> Result<NormalizedArtwork, ArtworkError> {
    if bytes.len() > MAX_SOURCE_BYTES {
        return Err(ArtworkError::TooLarge);
    }
    let decoded =
        image::load_from_memory(bytes).map_err(|e| ArtworkError::Decode(e.to_string()))?;
    let thumbnail = if decoded.width().max(decoded.height()) > MAX_THUMBNAIL_EDGE {
        decoded.thumbnail(MAX_THUMBNAIL_EDGE, MAX_THUMBNAIL_EDGE)
    } else {
        decoded
    };
    let mut png = Vec::new();
    thumbnail
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| ArtworkError::Encode(e.to_string()))?;
    let content_hash = hex::encode(Sha256::digest(&png));
    Ok(NormalizedArtwork { png, content_hash })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_png(width: u32, height: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_fn(width, height, |x, y| {
            image::Rgba([(x * 7 % 256) as u8, (y * 11 % 256) as u8, 128, 255])
        });
        let mut out = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    #[test]
    fn passthrough_small_image() {
        let source = sample_png(64, 48);
        let normalized = normalize_artwork(&source).unwrap();
        let decoded = image::load_from_memory(&normalized.png).unwrap();
        assert_eq!(decoded.width(), 64);
        assert_eq!(decoded.height(), 48);
        assert_eq!(normalized.content_hash.len(), 64);
        // 确定性：同输入同哈希
        assert_eq!(
            normalize_artwork(&source).unwrap().content_hash,
            normalized.content_hash
        );
    }

    #[test]
    fn downscales_to_512() {
        let source = sample_png(1024, 512);
        let normalized = normalize_artwork(&source).unwrap();
        let decoded = image::load_from_memory(&normalized.png).unwrap();
        assert!(decoded.width() <= 512 && decoded.height() <= 512);
        assert_eq!(decoded.height(), 256); // 1024×512 → 512×256 等比
    }

    #[test]
    fn rejects_oversize_and_garbage() {
        assert_eq!(
            normalize_artwork(&vec![0u8; MAX_SOURCE_BYTES + 1]).unwrap_err(),
            ArtworkError::TooLarge
        );
        assert!(matches!(
            normalize_artwork(b"not an image"),
            Err(ArtworkError::Decode(_))
        ));
    }
}
