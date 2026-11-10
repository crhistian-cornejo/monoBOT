//! The live item drawn into the menu bar icon's own image, so it reads as
//! part of the icon: monoBOT's mark, then the item's mark (a provider's
//! logo), then one or two small rows of text in the system font. The whole
//! image is a template, so the system tints it for a light or dark menu bar
//! as it does its own items. AppKit draws it again at each screen's scale.
use super::mark::{self, Pixels};
use block2::RcBlock;
use monocode_core::menu_bar::Item;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool};
use objc2::{AnyThread, Message};
use objc2_app_kit::{
    NSBitmapImageRep, NSColor, NSFont, NSFontAttributeName, NSForegroundColorAttributeName,
    NSImage, NSStringDrawing,
};
use objc2_foundation::{NSDictionary, NSPoint, NSRect, NSSize, NSString};

/// The image's height: the menu bar's room for an item's picture.
const HEIGHT: f64 = 22.;
/// monoBOT's mark, as the icon shows it alone.
const ICON: f64 = 18.;
/// The item's mark, and the gaps around it.
const MARK: f64 = 13.;
const GAP: f64 = 3.;
/// The rows: small enough that two fit the bar's height.
const FONT: f64 = 9.;
const ROW: f64 = 10.5;

/// The icon with the item beside it.
pub(crate) fn compose(icon: &NSImage, item: &Item) -> Retained<NSImage> {
    let font = NSFont::monospacedDigitSystemFontOfSize_weight(FONT, 0.23);
    let color = NSColor::blackColor();
    let attributes: Retained<NSDictionary<NSString, AnyObject>> = unsafe {
        NSDictionary::from_slices(
            &[NSFontAttributeName, NSForegroundColorAttributeName],
            &[&*font as &AnyObject, &*color as &AnyObject],
        )
    };
    let rows: Vec<Retained<NSString>> = item
        .rows
        .iter()
        .take(2)
        .map(|row| NSString::from_str(row))
        .collect();
    let text_width = rows
        .iter()
        .map(|row| unsafe { row.sizeWithAttributes(Some(&attributes)) }.width)
        .fold(0., f64::max)
        .ceil();
    let width = ICON + GAP + MARK + GAP + text_width + 1.;
    let mark = image(&mark::draw(item.mark, (MARK * 2.) as u32, None, [0, 0, 0]));
    let icon = icon.retain();
    let draw = RcBlock::new(move |_: NSRect| -> Bool {
        icon.drawInRect(NSRect::new(
            NSPoint::new(0., (HEIGHT - ICON) / 2.),
            NSSize::new(ICON, ICON),
        ));
        if let Some(mark) = &mark {
            mark.drawInRect(NSRect::new(
                NSPoint::new(ICON + GAP, (HEIGHT - MARK) / 2.),
                NSSize::new(MARK, MARK),
            ));
        }
        // Drawn flipped: y runs down from the top.
        let top = (HEIGHT - ROW * rows.len() as f64) / 2.;
        for (at, row) in rows.iter().enumerate() {
            unsafe {
                row.drawAtPoint_withAttributes(
                    NSPoint::new(ICON + GAP + MARK + GAP, top + ROW * at as f64 - 0.5),
                    Some(&attributes),
                );
            }
        }
        Bool::YES
    });
    let image = NSImage::imageWithSize_flipped_drawingHandler(
        NSSize::new(width.ceil(), HEIGHT),
        true,
        &draw,
    );
    image.setTemplate(true);
    image
}

/// Pixels as an image of half their size in points, for Retina screens.
fn image(pixels: &Pixels) -> Option<Retained<NSImage>> {
    let side = pixels.size as isize;
    let rep = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut(),
            side,
            side,
            8,
            4,
            true,
            false,
            &NSString::from_str("NSDeviceRGBColorSpace"),
            side * 4,
            32,
        )
    }?;
    let data = rep.bitmapData();
    if data.is_null() {
        return None;
    }
    // The representation holds premultiplied color.
    let premultiplied: Vec<u8> = pixels
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|px| {
            let alpha = px[3] as u32;
            let scale = |channel: u8| ((channel as u32 * alpha + 127) / 255) as u8;
            [scale(px[0]), scale(px[1]), scale(px[2]), px[3]]
        })
        .collect();
    unsafe {
        std::ptr::copy_nonoverlapping(premultiplied.as_ptr(), data, premultiplied.len());
    }
    let points = NSSize::new(pixels.size as f64 / 2., pixels.size as f64 / 2.);
    rep.setSize(points);
    let image = NSImage::initWithSize(NSImage::alloc(), points);
    image.addRepresentation(&rep);
    Some(image)
}
