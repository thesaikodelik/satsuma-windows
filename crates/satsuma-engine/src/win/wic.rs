//! Windows Imaging Component: decoding through every installed codec (HEIC, AVIF, RAW and
//! WebP with the Microsoft Store extensions) and HEIC encoding.

use super::{ensure_com, winerr};
use crate::{Error, Result};
use image::RgbaImage;
use std::path::Path;
use windows::core::{GUID, HSTRING, PCWSTR};
use windows::Win32::Foundation::GENERIC_READ;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::StructuredStorage::{IPropertyBag2, PROPBAG2};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::System::Variant::VARIANT;

fn factory() -> Result<IWICImagingFactory> {
    ensure_com();
    unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }
        .map_err(|e| winerr("Windows Imaging Component is unavailable", e))
}

/// EXIF orientation (1–8) of a decoded frame, if it has one.
fn orientation(frame: &IWICBitmapFrameDecode) -> Option<u16> {
    unsafe {
        let reader = frame.GetMetadataQueryReader().ok()?;
        let mut value = Default::default();
        reader.GetMetadataByName(&HSTRING::from("System.Photo.Orientation"), &mut value).ok()?;
        u16::try_from(&value).ok().filter(|o| (1..=8).contains(o))
    }
}

/// Decodes the first frame of any image WIC can read, upright.
pub fn decode(path: &Path) -> Result<RgbaImage> {
    let f = factory()?;
    let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let unreadable = |e| winerr(&format!("Could not read the image {name}"), e);
    unsafe {
        let decoder = f
            .CreateDecoderFromFilename(&HSTRING::from(path.as_os_str()), None, GENERIC_READ, WICDecodeMetadataCacheOnDemand)
            .map_err(unreadable)?;
        let frame = decoder.GetFrame(0).map_err(unreadable)?;
        let rgba = WICConvertBitmapSource(&GUID_WICPixelFormat32bppRGBA, &frame).map_err(unreadable)?;
        let (mut w, mut h) = (0u32, 0u32);
        rgba.GetSize(&mut w, &mut h).map_err(unreadable)?;
        let mut buf = vec![0u8; w as usize * h as usize * 4];
        rgba.CopyPixels(std::ptr::null(), w * 4, &mut buf).map_err(unreadable)?;
        let img = RgbaImage::from_raw(w, h, buf).ok_or_else(|| Error::msg(format!("Could not read the image {name}.")))?;
        Ok(match orientation(&frame) {
            Some(o) => super::raster::orient(img, o),
            None => img,
        })
    }
}

/// Whether an encoder for `container` is installed (HEIF needs the HEIF and HEVC extensions).
pub fn can_encode(container: &GUID) -> bool {
    let Ok(f) = factory() else { return false };
    let Ok(encoder) = (unsafe { f.CreateEncoder(container, std::ptr::null()) }) else { return false };
    // The HEIF container encoder exists even when no HEVC encoder is installed; try a frame.
    let Ok(stream) = (unsafe { f.CreateStream() }) else { return false };
    let Some(mem) = (unsafe { windows::Win32::UI::Shell::SHCreateMemStream(None) }) else { return false };
    unsafe {
        if stream.InitializeFromIStream(&mem).is_err() || encoder.Initialize(&stream, WICBitmapEncoderNoCache).is_err() {
            return false;
        }
        let mut frame = None;
        let mut props = None;
        if encoder.CreateNewFrame(&mut frame, &mut props).is_err() {
            return false;
        }
        let Some(frame) = frame else { return false };
        if frame.Initialize(props.as_ref()).is_err() || frame.SetSize(16, 16).is_err() {
            return false;
        }
        let img = RgbaImage::from_pixel(16, 16, image::Rgba([128, 128, 128, 255]));
        write_frame(&f, &frame, &img).is_ok() && frame.Commit().is_ok() && encoder.Commit().is_ok()
    }
}

/// HEIC writing, if this PC can do it.
pub fn can_encode_heif() -> bool {
    static CAN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *CAN.get_or_init(|| can_encode(&GUID_ContainerFormatHeif))
}

unsafe fn write_frame(f: &IWICImagingFactory, frame: &IWICBitmapFrameEncode, img: &RgbaImage) -> windows::core::Result<()> {
    let (w, h) = img.dimensions();
    let bitmap = f.CreateBitmapFromMemory(w, h, &GUID_WICPixelFormat32bppRGBA, w * 4, img.as_raw())?;
    frame.WriteSource(&bitmap, std::ptr::null())
}

fn set_quality(props: &IPropertyBag2, quality: f32) {
    let name: Vec<u16> = "ImageQuality\0".encode_utf16().collect();
    let bag = PROPBAG2 { pstrName: windows::core::PWSTR(name.as_ptr() as *mut u16), ..Default::default() };
    let value = VARIANT::from(quality.clamp(0.0, 1.0));
    unsafe {
        let _ = props.Write(1, &bag, &value);
    }
}

/// Encodes `img` into `container` (e.g. HEIF) at `quality` 0–1.
pub fn encode(img: &RgbaImage, path: &Path, container: &GUID, quality: f32) -> Result<()> {
    let f = factory()?;
    let fail = |e| winerr("Could not save the image", e);
    unsafe {
        let stream = f.CreateStream().map_err(fail)?;
        stream
            .InitializeFromFilename(PCWSTR(HSTRING::from(path.as_os_str()).as_ptr()), windows::Win32::Foundation::GENERIC_WRITE.0)
            .map_err(fail)?;
        let encoder = f.CreateEncoder(container, std::ptr::null()).map_err(fail)?;
        encoder.Initialize(&stream, WICBitmapEncoderNoCache).map_err(fail)?;
        let mut frame = None;
        let mut props = None;
        encoder.CreateNewFrame(&mut frame, &mut props).map_err(fail)?;
        let frame = frame.ok_or_else(|| Error::msg("Could not save the image."))?;
        if let Some(p) = &props {
            set_quality(p, quality);
        }
        frame.Initialize(props.as_ref()).map_err(fail)?;
        let (w, h) = img.dimensions();
        frame.SetSize(w, h).map_err(fail)?;
        write_frame(&f, &frame, img).map_err(fail)?;
        frame.Commit().map_err(fail)?;
        encoder.Commit().map_err(fail)?;
    }
    Ok(())
}
