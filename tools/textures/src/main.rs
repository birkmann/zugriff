//! `zg-tex`: source images → KTX2 textures the game loads as they are (no transcoding on the device).
//!
//! ```text
//! zg-tex color  <in.png|jpg> <out.ktx2> [--max 1024]            BC7, sRGB (albedo); mips filtered in linear light
//! zg-tex normal <in.png|jpg> <out.ktx2> [--max 1024] [--dx]     BC5 (RG), renormalised mips; --dx flips green
//! zg-tex orm    <ao|-> <roughness> <metal|-> <out.ktx2> [...]   BC7, linear: R = occlusion, G = roughness, B = metal
//! zg-tex rgba   <in.png> <out.ktx2> [--max 1024]            BC7, sRGB colour + straight alpha (decals); mips alpha-weighted
//! ```
//!
//! Every file carries its full mip chain, zstd-supercompressed. BC formats need desktop GPUs, which is what the game
//! targets (WebGPU's `texture-compression-bc` is available on desktop browsers). Load `orm` textures with
//! `is_srgb: false` — Bevy decides BC7's colour space from the loader settings, not from the file.

use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView};
use intel_tex_2::{RgSurface, RgbaSurface, bc5, bc7};

const VK_BC5_UNORM: u32 = 141;
const VK_BC7_UNORM: u32 = 145;
const VK_BC7_SRGB: u32 = 146;
const ZSTD: u32 = 2;
const ZSTD_LEVEL: i32 = 19;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Color,
    Normal,
    Orm,
    /// sRGB colour with straight (non-premultiplied) alpha, for alpha-blended decals.
    Rgba,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| args.iter().any(|a| a == name);
    let opt = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned());
    let max: u32 = opt("--max").and_then(|v| v.parse().ok()).unwrap_or(1024);
    let pos: Vec<&String> = {
        let mut skip = false;
        args.iter()
            .filter(|a| {
                if skip {
                    skip = false;
                    return false;
                }
                if a.as_str() == "--max" {
                    skip = true;
                    return false;
                }
                !a.starts_with("--")
            })
            .collect()
    };
    let (kind, rgba, out) = match pos.first().map(|s| s.as_str()) {
        Some("color") if pos.len() == 3 => (Kind::Color, open(pos[1]).to_rgba8(), pos[2]),
        Some("rgba") if pos.len() == 3 => (Kind::Rgba, open(pos[1]).to_rgba8(), pos[2]),
        Some("normal") if pos.len() == 3 => {
            let mut img = open(pos[1]).to_rgba8();
            if flag("--dx") {
                for p in img.pixels_mut() {
                    p[1] = 255 - p[1];
                }
            }
            (Kind::Normal, img, pos[2])
        }
        Some("orm") if pos.len() == 5 => {
            let rough = open(pos[2]).to_luma8();
            let (w, h) = rough.dimensions();
            let channel = |p: &str, fill: u8| -> Vec<u8> {
                if p == "-" {
                    vec![fill; (w * h) as usize]
                } else {
                    open(p).resize_exact(w, h, FilterType::Triangle).to_luma8().into_raw()
                }
            };
            let (ao, metal) = (channel(pos[1], 255), channel(pos[3], 0));
            let mut img = image::RgbaImage::new(w, h);
            for (i, p) in img.pixels_mut().enumerate() {
                *p = image::Rgba([ao[i], rough.as_raw()[i], metal[i], 255]);
            }
            (Kind::Orm, img, pos[4])
        }
        _ => {
            eprintln!("usage: zg-tex color|normal|rgba <in> <out.ktx2> [--max N] [--dx]\n       zg-tex orm <ao|-> <roughness> <metal|-> <out.ktx2> [--max N]");
            std::process::exit(2);
        }
    };

    // Scale down to the budget (keeping the aspect ratio, power-of-two friendly sizes stay so).
    let mut base = DynamicImage::ImageRgba8(rgba);
    let (w, h) = base.dimensions();
    if w.max(h) > max {
        let s = max as f32 / w.max(h) as f32;
        base = base.resize_exact(((w as f32 * s).round() as u32).max(1), ((h as f32 * s).round() as u32).max(1), FilterType::Lanczos3);
    }
    let base = base.to_rgba8();
    let (w, h) = base.dimensions();

    // The mip chain, filtered in the right space for the kind.
    let mut levels: Vec<(u32, u32, Vec<u8>)> = vec![(w, h, base.into_raw())];
    while levels.last().is_some_and(|l| l.0 > 1 || l.1 > 1) {
        let (pw, ph, prev) = levels.last().unwrap();
        levels.push(downsample(*pw, *ph, prev, kind));
    }

    let (vk, block_bytes) = match kind {
        Kind::Color | Kind::Rgba => (VK_BC7_SRGB, 16),
        Kind::Orm => (VK_BC7_UNORM, 16),
        Kind::Normal => (VK_BC5_UNORM, 16),
    };
    let settings = if kind == Kind::Rgba { bc7::alpha_basic_settings() } else { bc7::opaque_basic_settings() };
    let mut encoded: Vec<Vec<u8>> = Vec::new();
    for (lw, lh, px) in &levels {
        let (bw, bh, padded) = pad_to_blocks(*lw, *lh, px);
        let blocks = match kind {
            Kind::Normal => {
                let rg: Vec<u8> = padded.chunks_exact(4).flat_map(|p| [p[0], p[1]]).collect();
                bc5::compress_blocks(&RgSurface { data: &rg, width: bw, height: bh, stride: bw * 2 })
            }
            _ => bc7::compress_blocks(&settings, &RgbaSurface { data: &padded, width: bw, height: bh, stride: bw * 4 }),
        };
        encoded.push(blocks);
    }
    let file = ktx2(vk, w, h, &encoded, kind, block_bytes);
    std::fs::write(out, &file).unwrap_or_else(|e| fail(&format!("{out}: {e}")));
    let raw: usize = levels.iter().map(|l| l.2.len()).sum();
    println!("{out}: {w}×{h}, {} mips, {} KiB (RGBA8 would be {} KiB)", levels.len(), file.len() / 1024, raw / 1024);
}

fn open(path: &str) -> DynamicImage {
    image::open(path).unwrap_or_else(|e| fail(&format!("{path}: {e}")))
}

fn fail(msg: &str) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(1);
}

fn to_linear(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn to_srgb(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0).round() as u8
}

/// Halves an RGBA8 level with a 2×2 box filter: colour in linear light, normals renormalised, the rest as is.
fn downsample(w: u32, h: u32, src: &[u8], kind: Kind) -> (u32, u32, Vec<u8>) {
    if kind == Kind::Rgba {
        return downsample_alpha(w, h, src);
    }
    let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
    let mut out = vec![0u8; (nw * nh * 4) as usize];
    for y in 0..nh {
        for x in 0..nw {
            let mut acc = [0f32; 4];
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let sx = (x * 2 + dx).min(w - 1);
                let sy = (y * 2 + dy).min(h - 1);
                let i = ((sy * w + sx) * 4) as usize;
                for c in 0..4 {
                    acc[c] += match (kind, c) {
                        (Kind::Color, 0..=2) => to_linear(src[i + c]),
                        (Kind::Normal, 0..=2) => src[i + c] as f32 / 255.0 * 2.0 - 1.0,
                        _ => src[i + c] as f32 / 255.0,
                    };
                }
            }
            for a in &mut acc {
                *a *= 0.25;
            }
            if kind == Kind::Normal {
                let l = (acc[0] * acc[0] + acc[1] * acc[1] + acc[2] * acc[2]).sqrt().max(1e-6);
                for a in &mut acc[..3] {
                    *a /= l;
                }
            }
            let o = ((y * nw + x) * 4) as usize;
            for c in 0..4 {
                out[o + c] = match (kind, c) {
                    (Kind::Color, 0..=2) => to_srgb(acc[c]),
                    (Kind::Normal, 0..=2) => ((acc[c] * 0.5 + 0.5) * 255.0).round().clamp(0.0, 255.0) as u8,
                    _ => (acc[c] * 255.0).round().clamp(0.0, 255.0) as u8,
                };
            }
        }
    }
    (nw, nh, out)
}

/// Halves a straight-alpha RGBA8 level: colour averaged in linear light weighted by alpha (so transparent texels
/// don't darken or tint the edges), alpha averaged as is. Fully transparent 2×2 groups keep their plain colour
/// average, so colour dilated into the transparent padding survives down the chain.
fn downsample_alpha(w: u32, h: u32, src: &[u8]) -> (u32, u32, Vec<u8>) {
    let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
    let mut out = vec![0u8; (nw * nh * 4) as usize];
    for y in 0..nh {
        for x in 0..nw {
            let (mut weighted, mut plain, mut alpha) = ([0f32; 3], [0f32; 3], 0f32);
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let sx = (x * 2 + dx).min(w - 1);
                let sy = (y * 2 + dy).min(h - 1);
                let i = ((sy * w + sx) * 4) as usize;
                let a = src[i + 3] as f32 / 255.0;
                for c in 0..3 {
                    let l = to_linear(src[i + c]);
                    weighted[c] += l * a;
                    plain[c] += l;
                }
                alpha += a;
            }
            let o = ((y * nw + x) * 4) as usize;
            for c in 0..3 {
                let l = if alpha > 1e-4 { weighted[c] / alpha } else { plain[c] * 0.25 };
                out[o + c] = to_srgb(l);
            }
            out[o + 3] = (alpha * 0.25 * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
    (nw, nh, out)
}

/// Pads a level to whole 4×4 blocks by repeating its edge.
fn pad_to_blocks(w: u32, h: u32, px: &[u8]) -> (u32, u32, Vec<u8>) {
    let (bw, bh) = (w.div_ceil(4) * 4, h.div_ceil(4) * 4);
    if (bw, bh) == (w, h) {
        return (w, h, px.to_vec());
    }
    let mut out = vec![0u8; (bw * bh * 4) as usize];
    for y in 0..bh {
        for x in 0..bw {
            let (sx, sy) = (x.min(w - 1), y.min(h - 1));
            let (s, d) = (((sy * w + sx) * 4) as usize, ((y * bw + x) * 4) as usize);
            out[d..d + 4].copy_from_slice(&px[s..s + 4]);
        }
    }
    (bw, bh, out)
}

/// A KTX2 file: header, level index, a basic data format descriptor, then the zstd levels smallest first.
fn ktx2(vk: u32, w: u32, h: u32, levels: &[Vec<u8>], kind: Kind, block_bytes: u8) -> Vec<u8> {
    let compressed: Vec<Vec<u8>> = levels.iter().map(|l| zstd::bulk::compress(l, ZSTD_LEVEL).expect("zstd")).collect();

    // Data format descriptor (KDFS basic block).
    let samples: Vec<[u32; 4]> = match kind {
        // One 128-bit colour sample.
        Kind::Color | Kind::Orm | Kind::Rgba => vec![[(127 << 16), 0, 0, u32::MAX]],
        // Red then green, 64 bits each.
        Kind::Normal => vec![[(63 << 16), 0, 0, u32::MAX], [64 | (63 << 16) | (1 << 24), 0, 0, u32::MAX]],
    };
    let color_model: u32 = if kind == Kind::Normal { 132 } else { 134 };
    let transfer: u32 = if matches!(kind, Kind::Color | Kind::Rgba) { 2 } else { 1 };
    let block_size = 24 + 16 * samples.len() as u32;
    let mut dfd = Vec::new();
    let push = |v: &mut Vec<u8>, x: u32| v.extend_from_slice(&x.to_le_bytes());
    push(&mut dfd, 4 + block_size);
    push(&mut dfd, 0); // vendor 0 (Khronos), descriptor type 0 (basic)
    push(&mut dfd, 2 | (block_size << 16)); // version 2
    push(&mut dfd, color_model | (1 << 8) | (transfer << 16)); // BT.709 primaries, straight alpha
    push(&mut dfd, 3 | (3 << 8)); // 4×4×1×1 blocks (stored minus one)
    push(&mut dfd, block_bytes as u32); // bytesPlane0
    push(&mut dfd, 0);
    for s in &samples {
        for x in s {
            push(&mut dfd, *x);
        }
    }

    let header_len = 12 + 9 * 4 + 4 * 4 + 2 * 8;
    let index_len = levels.len() * 24;
    let dfd_offset = header_len + index_len;
    let mut data_offset = dfd_offset + dfd.len();
    data_offset = data_offset.div_ceil(8) * 8;

    // Level data goes smallest mip first; the index lists level 0 first.
    let mut offsets = vec![0usize; levels.len()];
    let mut at = data_offset;
    for i in (0..levels.len()).rev() {
        offsets[i] = at;
        at += compressed[i].len();
    }

    let mut f = Vec::with_capacity(at);
    f.extend_from_slice(&[0xAB, 0x4B, 0x54, 0x58, 0x20, 0x32, 0x30, 0xBB, 0x0D, 0x0A, 0x1A, 0x0A]);
    for x in [vk, 1, w, h, 0, 0, 1, levels.len() as u32, ZSTD] {
        push(&mut f, x);
    }
    for x in [dfd_offset as u32, dfd.len() as u32, 0, 0] {
        push(&mut f, x);
    }
    f.extend_from_slice(&0u64.to_le_bytes());
    f.extend_from_slice(&0u64.to_le_bytes());
    for i in 0..levels.len() {
        f.extend_from_slice(&(offsets[i] as u64).to_le_bytes());
        f.extend_from_slice(&(compressed[i].len() as u64).to_le_bytes());
        f.extend_from_slice(&(levels[i].len() as u64).to_le_bytes());
    }
    f.extend_from_slice(&dfd);
    f.resize(data_offset, 0);
    for i in (0..levels.len()).rev() {
        f.extend_from_slice(&compressed[i]);
    }
    f
}
