//! Finds apps for some window classes and renders their icons at board size into one PNG:
//! `cargo run -p openpager-desktop --example app_icons -- out.png firefox code Claude ...`
#[cfg(target_os = "linux")]
fn main() {
    use openpager_desktop::apps;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let installed = apps::installed();
    println!("{} apps installed", installed.len());
    let mut sheet = image::RgbaImage::from_pixel(40 * (args.len() as u32 - 1), 40, image::Rgba([30, 36, 44, 255]));
    for (i, class) in args[1..].iter().enumerate() {
        let Some(app) = apps::for_window(&installed, class, class) else { println!("{class}: no app"); continue };
        let file = apps::icon_file(&app.icon);
        println!("{class}: {} ({}) icon {:?} -> {:?}", app.name, app.id, app.icon, file);
        if let Some(pixels) = file.as_deref().and_then(|f| apps::render(f, 32)) {
            let icon = image::RgbaImage::from_raw(32, 32, pixels).unwrap();
            image::imageops::overlay(&mut sheet, &icon, 4 + 40 * i as i64, 4);
        }
    }
    sheet.save(&args[0]).unwrap();
}
#[cfg(not(target_os = "linux"))]
fn main() {}
