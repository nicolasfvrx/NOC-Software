use std::{os::windows::ffi::OsStrExt, path::Path};
use windows::{
    core::*,
    Win32::{Graphics::Imaging::*, System::Com::*},
};

pub struct Assets {
    pub background: Option<IWICFormatConverter>,
    pub logo: Option<IWICFormatConverter>,
}

impl Assets {
    pub fn load() -> Self {
        let mut assets = Self {
            background: None,
            logo: None,
        };
        let result = (|| -> Result<()> {
            let dir = crate::executable_dir().map_err(|e| {
                Error::new(windows::Win32::Foundation::E_FAIL, e.to_string().into())
            })?;
            let factory: IWICImagingFactory =
                unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)? };
            assets.background = load_optional(&factory, &dir.join("background.jpg"));
            assets.logo = load_optional(&factory, &dir.join("logo.png"));
            Ok(())
        })();
        if let Err(error) = result {
            crate::log_error(error);
        }
        assets
    }
}

fn load_optional(factory: &IWICImagingFactory, path: &Path) -> Option<IWICFormatConverter> {
    match unsafe { load_image(factory, path) } {
        Ok(image) => Some(image),
        Err(error) => {
            crate::log_error(format!("{}: {}", path.display(), error));
            None
        }
    }
}

unsafe fn load_image(factory: &IWICImagingFactory, path: &Path) -> Result<IWICFormatConverter> {
    let filename: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let decoder = factory.CreateDecoderFromFilename(
        PCWSTR(filename.as_ptr()),
        None,
        windows::Win32::Foundation::GENERIC_READ,
        WICDecodeMetadataCacheOnLoad,
    )?;
    let frame = decoder.GetFrame(0)?;
    // Premultiplied BGRA is essential for correct transparent PNG compositing.
    let converter = factory.CreateFormatConverter()?;
    converter.Initialize(
        &frame,
        &GUID_WICPixelFormat32bppPBGRA,
        WICBitmapDitherTypeNone,
        None,
        0.0,
        WICBitmapPaletteTypeCustom,
    )?;
    Ok(converter)
}
