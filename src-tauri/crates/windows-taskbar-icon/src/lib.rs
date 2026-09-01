#![cfg(windows)]

//! Safe ownership boundary for a dynamically generated Windows taskbar icon.

use std::{error::Error, ffi::c_void, fmt, mem::size_of, ptr};
use windows_sys::Win32::{
    Foundation::HWND,
    Graphics::Gdi::{
        CreateBitmap, CreateDIBSection, DeleteObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
        DIB_RGB_COLORS,
    },
    UI::WindowsAndMessaging::{
        CreateIconIndirect, DestroyIcon, SendMessageW, ICONINFO, ICON_BIG, WM_SETICON,
    },
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskbarIconError {
    InvalidImage,
    NativeCallFailed,
}

impl fmt::Display for TaskbarIconError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidImage => formatter.write_str("invalid taskbar icon image"),
            Self::NativeCallFailed => formatter.write_str("Windows rejected the taskbar icon"),
        }
    }
}

impl Error for TaskbarIconError {}

/// Owns the `HICON` installed as `ICON_BIG` on a window.
///
/// The handle must remain alive while Windows displays it. Replacing this value
/// only after installing the next icon ensures that the old handle is destroyed
/// at the correct time.
#[derive(Debug)]
pub struct TaskbarIcon {
    handle: usize,
}

impl TaskbarIcon {
    pub fn set_for_window(
        hwnd: isize,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> Result<Self, TaskbarIconError> {
        let pixel_count = width
            .checked_mul(height)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or(TaskbarIconError::InvalidImage)? as usize;
        if hwnd == 0 || width == 0 || height == 0 || rgba.len() != pixel_count {
            return Err(TaskbarIconError::InvalidImage);
        }
        let width = i32::try_from(width).map_err(|_| TaskbarIconError::InvalidImage)?;
        let height = i32::try_from(height).map_err(|_| TaskbarIconError::InvalidImage)?;
        let bgra = rgba_to_bgra(rgba);

        let bitmap_info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                biSizeImage: bgra.len() as u32,
                ..BITMAPINFOHEADER::default()
            },
            ..BITMAPINFO::default()
        };
        let mut bitmap_bits = ptr::null_mut::<c_void>();
        let color_bitmap = unsafe {
            CreateDIBSection(
                ptr::null_mut(),
                &bitmap_info,
                DIB_RGB_COLORS,
                &mut bitmap_bits,
                ptr::null_mut(),
                0,
            )
        };
        if color_bitmap.is_null() || bitmap_bits.is_null() {
            return Err(TaskbarIconError::NativeCallFailed);
        }
        unsafe {
            ptr::copy_nonoverlapping(bgra.as_ptr(), bitmap_bits.cast::<u8>(), bgra.len());
        }

        let mask_stride = (width as usize).div_ceil(16) * 2;
        let mask = vec![0_u8; mask_stride * height as usize];
        let mask_bitmap =
            unsafe { CreateBitmap(width, height, 1, 1, mask.as_ptr().cast::<c_void>()) };
        if mask_bitmap.is_null() {
            unsafe {
                DeleteObject(color_bitmap);
            }
            return Err(TaskbarIconError::NativeCallFailed);
        }

        let icon_info = ICONINFO {
            fIcon: 1,
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask_bitmap,
            hbmColor: color_bitmap,
        };
        let icon = unsafe { CreateIconIndirect(&icon_info) };
        unsafe {
            DeleteObject(mask_bitmap);
            DeleteObject(color_bitmap);
        }
        if icon.is_null() {
            return Err(TaskbarIconError::NativeCallFailed);
        }

        unsafe {
            SendMessageW(hwnd as HWND, WM_SETICON, ICON_BIG as usize, icon as isize);
        }
        Ok(Self {
            handle: icon as usize,
        })
    }
}

impl Drop for TaskbarIcon {
    fn drop(&mut self) {
        if self.handle != 0 {
            unsafe {
                DestroyIcon(self.handle as _);
            }
        }
    }
}

fn rgba_to_bgra(rgba: &[u8]) -> Vec<u8> {
    rgba.chunks_exact(4)
        .flat_map(|pixel| [pixel[2], pixel[1], pixel[0], pixel[3]])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_renderer_rgba_to_windows_bgra_without_losing_alpha() {
        assert_eq!(
            rgba_to_bgra(&[10, 20, 30, 40, 200, 150, 100, 50]),
            vec![30, 20, 10, 40, 100, 150, 200, 50]
        );
    }

    #[test]
    fn rejects_inconsistent_pixel_buffers_before_calling_windows() {
        assert!(matches!(
            TaskbarIcon::set_for_window(1, &[255, 255, 255], 1, 1),
            Err(TaskbarIconError::InvalidImage)
        ));
    }
}
