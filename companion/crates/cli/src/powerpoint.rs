//! PowerPoint presenter notes: live Windows slide state and unsaved notes through a
//! read-only worker, with saved-PPTX parsing as a cross-platform fallback.

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use knoend_core::focus::Focus;
use zip::ZipArchive;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SlideNotes {
    pub file_name: String,
    pub is_presenting: bool,
    pub slide_index: u32,
    pub total_slides: u32,
    pub note_page: usize,
    pub total_note_pages: usize,
    pub notes: String,
}

#[derive(Debug, Clone, Default)]
pub struct ActivePresentation {
    pub file_name: String,
    pub total_slides: u32,
    pub current_slide: u32,
    pub note_page: usize,
    pub notes: HashMap<u32, Vec<String>>,
}

static ACTIVE: Mutex<Option<ActivePresentation>> = Mutex::new(None);
static MANUAL_PRESENTING: Mutex<bool> = Mutex::new(false);
/// Sets whether presentation mode was triggered manually (e.g. Present tapped on board).
pub fn set_presenting(presenting: bool) {
    if let Ok(mut lock) = MANUAL_PRESENTING.lock() {
        *lock = presenting;
    }
    if !presenting {
        clear();
    }
}

/// Whether presentation mode is currently active (either slide show window focused or manually triggered).
pub fn is_active_presenting(title: &str) -> bool {
    // Windows observes the real show window, including shows started with the PC
    // keyboard and shows ended outside the device. A manual latch would outlive Esc.
    #[cfg(target_os = "windows")]
    { live_slide().filter(|s| s.available).map_or_else(|| is_presenting(title), |s| s.presenting) }
    #[cfg(not(target_os = "windows"))]
    { is_presenting(title) || MANUAL_PRESENTING.lock().map_or(false, |p| *p) }
}

/// Called when PowerPoint has focus: returns slide notes ONLY if PowerPoint is in presentation mode.
pub fn get_notes(focus: &Focus) -> Option<SlideNotes> { get_notes_with_source(focus,"live_or_pptx") }

pub fn get_notes_with_source(focus:&Focus,recipe_source:&str)->Option<SlideNotes> {
    let title = &focus.title;
    if !is_active_presenting(title) {
        clear();
        return None;
    }

    #[cfg(windows)]
    {
        let settings=notes_settings();let source=if settings.source=="recipe"{recipe_source}else{&settings.source};let live=live_slide();
        let (name,total,index)=live.as_ref().filter(|s|s.available&&s.presenting).map(|s|(s.name.clone(),s.total,s.slide)).unwrap_or_else(||{
            let previous=ACTIVE.lock().unwrap();("PowerPoint".into(),previous.as_ref().map_or(0,|p|p.total_slides),previous.as_ref().map_or(1,|p|p.current_slide))
        });
        if source!="pptx" {
            if let Some(live)=live.as_ref().filter(|s|s.available&&s.presenting&&s.notes_available) {
                let mut lock=ACTIVE.lock().ok()?;*lock=Some(presentation_from_live(live,lock.as_ref()));drop(lock);return current_slide_notes();
            }
        }
        let file=if !settings.file.is_empty(){Some(PathBuf::from(&settings.file))}else{live.as_ref().filter(|s|s.available&&s.presenting).map(|s|PathBuf::from(&s.path))};
        let error=if source=="live"{"Live PowerPoint notes are unavailable."}
        else if let Some(path)=file.filter(|p|p.is_absolute()&&p.extension().is_some_and(|x|x.eq_ignore_ascii_case("pptx"))) {
            if let Some(active)=live.as_ref().filter(|s|s.available&&s.presenting&&Path::new(&s.path).is_absolute()) {
                if !same_file(&path,Path::new(&active.path)){return Some(unavailable(&name,total,index,"The configured PPTX does not match the active presentation."));}
            }
            if let Some(mut pres)=cached_file(&path,&name) {
                pres.current_slide=index;let mut lock=ACTIVE.lock().ok()?;
                pres.note_page=lock.as_ref().filter(|p|p.file_name==name&&p.current_slide==index).map_or(0,|p|p.note_page);
                *lock=Some(pres);drop(lock);return current_slide_notes();
            }
            "The exact saved PPTX cannot be read. Check the file binding."
        }else{"Live notes are unavailable; bind an exact saved PPTX to use file extraction."};
        return Some(unavailable(&name,total,index,error));
    }
    #[cfg(not(windows))]
    {
    let file_name = extract_presentation_name(title)
        .unwrap_or_else(|| "Presentation".to_string());
    let mut lock = ACTIVE.lock().ok()?;

    let needs_load = lock.as_ref().map_or(true, |a| a.file_name != file_name);
    if needs_load {
        if let Some(pres) = load_presentation(&file_name) {
            *lock = Some(pres);
        } else {
            // Unsaved presentation, new template, or file not on disk:
            // Still create an ActivePresentation so presentation controls and slide navigation WORK!
            *lock = Some(ActivePresentation {
                file_name: file_name.clone(),
                total_slides: 0,
                current_slide: 1,
                note_page: 0,
                notes: HashMap::new(),
            });
        }
    }

    let active = lock.as_mut()?;
    let current_slide = if active.total_slides > 0 {
        active.current_slide.clamp(1, active.total_slides)
    } else {
        active.current_slide.max(1)
    };
    let empty_vec = Vec::new();
    let pages = active.notes.get(&current_slide).unwrap_or(&empty_vec);
    let total_pages = pages.len().max(1);
    let note_page = active.note_page.min(total_pages - 1);
    let notes = pages.get(note_page).cloned().unwrap_or_else(|| "No notes for this slide".to_string());

    Some(SlideNotes {
        file_name: active.file_name.clone(),
        is_presenting: true,
        slide_index: current_slide,
        total_slides: active.total_slides,
        note_page,
        total_note_pages: total_pages,
        notes,
    })
    }
}

pub fn current_slide_notes() -> Option<SlideNotes> {
    let lock = ACTIVE.lock().ok()?;
    let active = lock.as_ref()?;
    let current_slide = if active.total_slides > 0 {
        active.current_slide.clamp(1, active.total_slides)
    } else {
        active.current_slide.max(1)
    };
    let empty_vec = Vec::new();
    let pages = active.notes.get(&current_slide).unwrap_or(&empty_vec);
    let total_pages = pages.len().max(1);
    let note_page = active.note_page.min(total_pages - 1);
    let notes = pages.get(note_page).cloned().unwrap_or_else(|| "No notes for this slide".to_string());

    Some(SlideNotes {
        file_name: active.file_name.clone(),
        is_presenting: true,
        slide_index: current_slide,
        total_slides: active.total_slides,
        note_page,
        total_note_pages: total_pages,
        notes,
    })
}

/// Whether the window title indicates PowerPoint is in Slide Show (presentation) mode.
pub fn is_presenting(title: &str) -> bool {
    let lower = title.to_ascii_lowercase();
    lower.contains("slide show")
        || lower.contains("diaporama")
        || lower.contains("presenter view")
        || lower.starts_with("powerpoint slide show")
}

/// Advance to next slide.
pub fn next_slide() -> Option<SlideNotes> {
    #[cfg(windows)]
    if live_slide().is_some_and(|s| s.available && s.presenting) { return current_slide_notes(); }

    let mut lock = ACTIVE.lock().ok()?;
    let active = lock.as_mut()?;
    if active.total_slides == 0 || active.current_slide < active.total_slides {
        active.current_slide += 1;
        active.note_page = 0;
    }
    let current = active.current_slide;
    let empty_vec = Vec::new();
    let pages = active.notes.get(&current).unwrap_or(&empty_vec);
    let total_pages = pages.len().max(1);
    let notes = pages.get(0).cloned().unwrap_or_else(|| "No notes for this slide".to_string());

    Some(SlideNotes {
        file_name: active.file_name.clone(),
        is_presenting: true,
        slide_index: current,
        total_slides: active.total_slides,
        note_page: 0,
        total_note_pages: total_pages,
        notes,
    })
}

/// Go to previous slide.
pub fn prev_slide() -> Option<SlideNotes> {
    #[cfg(windows)]
    if live_slide().is_some_and(|s| s.available && s.presenting) { return current_slide_notes(); }

    let mut lock = ACTIVE.lock().ok()?;
    let active = lock.as_mut()?;
    if active.current_slide > 1 {
        active.current_slide -= 1;
        active.note_page = 0;
    }
    let current = active.current_slide;
    let empty_vec = Vec::new();
    let pages = active.notes.get(&current).unwrap_or(&empty_vec);
    let total_pages = pages.len().max(1);
    let notes = pages.get(0).cloned().unwrap_or_else(|| "No notes for this slide".to_string());

    Some(SlideNotes {
        file_name: active.file_name.clone(),
        is_presenting: true,
        slide_index: current,
        total_slides: active.total_slides,
        note_page: 0,
        total_note_pages: total_pages,
        notes,
    })
}

/// Go to first slide.
pub fn first_slide() -> Option<SlideNotes> {
    #[cfg(windows)]
    if live_slide().is_some_and(|s| s.available && s.presenting) { return current_slide_notes(); }

    let mut lock = ACTIVE.lock().ok()?;
    let active = lock.as_mut()?;
    active.current_slide = 1;
    active.note_page = 0;
    let empty_vec = Vec::new();
    let pages = active.notes.get(&1).unwrap_or(&empty_vec);
    let total_pages = pages.len().max(1);
    let notes = pages.get(0).cloned().unwrap_or_else(|| "No notes for this slide".to_string());

    Some(SlideNotes {
        file_name: active.file_name.clone(),
        is_presenting: true,
        slide_index: 1,
        total_slides: active.total_slides,
        note_page: 0,
        total_note_pages: total_pages,
        notes,
    })
}

/// Cycle to the next page of notes for the current slide.
pub fn next_note_page() -> Option<SlideNotes> {
    let mut lock = ACTIVE.lock().ok()?;
    let active = lock.as_mut()?;
    let current_slide = active.current_slide;
    let empty_vec = Vec::new();
    let pages = active.notes.get(&current_slide).unwrap_or(&empty_vec);
    let total_pages = pages.len().max(1);
    if total_pages > 1 {
        active.note_page = (active.note_page + 1) % total_pages;
    }
    let note_page = active.note_page;
    let notes = pages.get(note_page).cloned().unwrap_or_else(|| "No notes for this slide".to_string());

    Some(SlideNotes {
        file_name: active.file_name.clone(),
        is_presenting: true,
        slide_index: current_slide,
        total_slides: active.total_slides,
        note_page,
        total_note_pages: total_pages,
        notes,
    })
}

/// Cycle to the previous page of notes for the current slide.
pub fn prev_note_page() -> Option<SlideNotes> {
    let mut lock = ACTIVE.lock().ok()?;
    let active = lock.as_mut()?;
    let current_slide = active.current_slide;
    let empty_vec = Vec::new();
    let pages = active.notes.get(&current_slide).unwrap_or(&empty_vec);
    let total_pages = pages.len().max(1);
    if total_pages > 1 {
        active.note_page = if active.note_page == 0 { total_pages - 1 } else { active.note_page - 1 };
    }
    let note_page = active.note_page;
    let notes = pages.get(note_page).cloned().unwrap_or_else(|| "No notes for this slide".to_string());

    Some(SlideNotes {
        file_name: active.file_name.clone(),
        is_presenting: true,
        slide_index: current_slide,
        total_slides: active.total_slides,
        note_page,
        total_note_pages: total_pages,
        notes,
    })
}

/// Clears presentation from memory when PowerPoint closes or presentation ends.
pub fn clear() {
    crate::presentation::reset();
    if let Ok(mut lock) = ACTIVE.lock() {
        *lock = None;
    }
    if let Ok(mut lock) = MANUAL_PRESENTING.lock() {
        *lock = false;
    }
}

/// Extracts presentation file name from window titles.
pub fn extract_presentation_name(title: &str) -> Option<String> {
    let cleaned_title = title.replace("[Protected View]", "");
    let title = cleaned_title.trim();
    let lower = title.to_ascii_lowercase();

    // Slide show mode e.g. "PowerPoint Slide Show - [filename.pptx]" or "PowerPoint Slide Show - Presentation1"
    if lower.contains("slide show") || lower.contains("diaporama") {
        if let (Some(open), Some(close)) = (title.find('['), title.rfind(']')) {
            if open < close {
                let inside = title[open + 1..close].trim().trim_matches('[').trim_matches(']');
                if !inside.is_empty() {
                    return Some(inside.to_string());
                }
            }
        }
        let after_show = if let Some(idx) = lower.find("slide show") {
            &title[idx + 10..]
        } else if let Some(idx) = lower.find("diaporama") {
            &title[idx + 9..]
        } else {
            ""
        };
        let cleaned = after_show.trim_start_matches(|c| c == ' ' || c == '-' || c == ':').trim();
        let name = cleaned.trim_matches('[').trim_matches(']').trim();
        if !name.is_empty() {
            return Some(name.to_string());
        }
    }

    if let Some(pptx_idx) = lower.find(".pptx") {
        let end = pptx_idx + 5;
        let candidate = &title[..end];
        let name = candidate.trim().trim_start_matches('[').trim();
        if !name.is_empty() {
            return Some(name.to_string());
        }
    }

    // Try strip " - PowerPoint" suffix if no .pptx extension is displayed in title
    if let Some(pos) = lower.find(" - powerpoint") {
        let candidate = &title[..pos];
        let name = candidate.trim().trim_start_matches('[').trim();
        if !name.is_empty() {
            return Some(name.to_string());
        }
    }

    None
}

/// Looks for the PPTX file in standard user folders (Downloads, Documents, Desktop, etc.).
pub fn find_pptx_file(name: &str) -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)?;

    let candidates = [
        name.to_string(),
        if !name.ends_with(".pptx") { format!("{name}.pptx") } else { name.to_string() },
    ];

    let search_dirs = [
        home.join("Downloads"),
        home.join("Documents"),
        home.join("Desktop"),
        home.join("OneDrive").join("Documents"),
        home.join("OneDrive").join("Desktop"),
        home.join("OneDrive").join("Downloads"),
    ];

    for candidate in &candidates {
        for dir in &search_dirs {
            let path = dir.join(candidate);
            if path.is_file() {
                return Some(path);
            }
        }
    }

    // Check AppData\Microsoft\Office\Recent
    if let Some(appdata) = std::env::var_os("APPDATA").map(PathBuf::from) {
        let recent_dir = appdata.join("Microsoft").join("Office").join("Recent");
        if recent_dir.is_dir() {
            for candidate in &candidates {
                let lnk_name = format!("{candidate}.LNK");
                let lnk_path = recent_dir.join(&lnk_name);
                if lnk_path.is_file() {
                    if let Ok(bytes) = std::fs::read(&lnk_path) {
                        if let Some(target) = extract_path_from_lnk(&bytes) {
                            if target.is_file() {
                                return Some(target);
                            }
                        }
                    }
                }
            }
        }
    }

    None
}

fn extract_path_from_lnk(bytes: &[u8]) -> Option<PathBuf> {
    let s = String::from_utf8_lossy(bytes);
    for part in s.split('\0') {
        if part.len() > 3 && (part.contains(":\\") || part.contains(":/")) && part.ends_with(".pptx") {
            let path = PathBuf::from(part);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

/// Loads a presentation from disk directly into memory and extracts notes for each slide.
pub fn load_presentation(file_name: &str) -> Option<ActivePresentation> {
    let path = find_pptx_file(file_name)?;
    load_presentation_from_path(&path, file_name)
}

pub fn load_presentation_from_path(path: &Path, file_name: &str) -> Option<ActivePresentation> {
    let file = File::open(path).ok()?;
    let mut archive = ZipArchive::new(file).ok()?;

    // Presentation order follows sldIdLst relationships, never slide filenames.
    let presentation_xml=zip_xml(&mut archive,"ppt/presentation.xml")?;
    let rels_xml=zip_xml(&mut archive,"ppt/_rels/presentation.xml.rels")?;
    let doc=roxmltree::Document::parse(&presentation_xml).ok()?;
    let rels=roxmltree::Document::parse(&rels_xml).ok()?;
    let mut slide_paths=Vec::new();
    for slide in doc.descendants().filter(|n|n.has_tag_name(("http://schemas.openxmlformats.org/presentationml/2006/main","sldId"))) {
        let id=slide.attribute(("http://schemas.openxmlformats.org/officeDocument/2006/relationships","id"))?;
        let rel=rels.descendants().find(|n|n.attribute("Id")==Some(id)&&n.attribute("Type").is_some_and(|t|t.ends_with("/slide")))?;
        if rel.attribute("TargetMode")==Some("External"){return None}
        slide_paths.push(package_target("ppt/presentation.xml",rel.attribute("Target")?)?);
    }
    if slide_paths.len()>10000{return None}
    let total_slides=slide_paths.len() as u32;
    let mut notes=HashMap::new();
    for (index,path) in slide_paths.iter().enumerate(){
        let (dir,name)=path.rsplit_once('/')?;
        let Some(xml)=zip_xml(&mut archive,&format!("{dir}/_rels/{name}.rels")) else{continue};
        let Ok(rels)=roxmltree::Document::parse(&xml) else{continue};
        let Some(rel)=rels.descendants().find(|n|n.attribute("Type").is_some_and(|t|t.ends_with("/notesSlide"))) else{continue};
        if rel.attribute("TargetMode")==Some("External"){continue}
        let Some(target)=rel.attribute("Target").and_then(|t|package_target(path,t)) else{continue};
        if let Some(xml)=zip_xml(&mut archive,&target){notes.insert(index as u32+1,paginate_notes(&parse_notes_xml(&xml),220));}
    }

    Some(ActivePresentation {
        file_name: file_name.to_string(),
        total_slides,
        current_slide: 1,
        note_page: 0,
        notes,
    })
}

fn zip_xml(archive:&mut ZipArchive<File>,name:&str)->Option<String>{
    let file=archive.by_name(name).ok()?;if file.size()>4*1024*1024{return None}
    let mut text=String::new();file.take(4*1024*1024+1).read_to_string(&mut text).ok()?;Some(text)
}
fn package_target(source:&str,target:&str)->Option<String>{
    if target.contains(['\\',':','?','#']){return None}
    let mut parts:Vec<&str>=if target.starts_with('/'){Vec::new()}else{source.rsplit_once('/')?.0.split('/').collect()};
    for part in target.split('/') {match part{""|"."=>{},".."=>{parts.pop()?;},part=>parts.push(part)}}
    Some(parts.join("/"))
}

/// Chunks slide notes into pages of at most ~220 characters, breaking cleanly on lines or word boundaries.
pub fn paginate_notes(raw_notes: &str, max_chars: usize) -> Vec<String> {
    let clean_ascii: String = raw_notes.chars()
        .map(|c| match c {
            '\u{2018}' | '\u{2019}' => '\'',
            '\u{201C}' | '\u{201D}' => '"',
            '\u{2013}' | '\u{2014}' => '-',
            '\u{2022}' => '*',
            _ if c == '\n' || c.is_ascii_graphic() || c == ' ' => c,
            _ => ' ',
        })
        .collect();
    let text = clean_ascii.trim();
    if text.is_empty() {
        return vec!["No notes for this slide".to_string()];
    }

    let mut pages = Vec::new();
    let mut current_page = String::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if !current_page.is_empty() && current_page.len() + 1 + line.len() > max_chars {
            pages.push(current_page.trim().to_string());
            current_page = String::new();
        }
        if line.len() > max_chars {
            for word in line.split_whitespace() {
                if !current_page.is_empty() && current_page.len() + 1 + word.len() > max_chars {
                    pages.push(current_page.trim().to_string());
                    current_page = String::new();
                }
                if !current_page.is_empty() {
                    current_page.push(' ');
                }
                current_page.push_str(word);
            }
        } else {
            if !current_page.is_empty() {
                current_page.push('\n');
            }
            current_page.push_str(line);
        }
    }
    if !current_page.trim().is_empty() {
        pages.push(current_page.trim().to_string());
    }
    if pages.is_empty() {
        pages.push("No notes for this slide".to_string());
    }
    pages
}

fn parse_notes_xml(xml:&str)->String{
    let Ok(doc)=roxmltree::Document::parse(xml) else{return String::new()};
    let mut paragraphs=Vec::new();
    for shape in doc.descendants().filter(|n|n.has_tag_name(("http://schemas.openxmlformats.org/presentationml/2006/main","sp"))) {
        let body=shape.descendants().any(|n|n.has_tag_name(("http://schemas.openxmlformats.org/presentationml/2006/main","ph"))&&n.attribute("type")==Some("body"));
        if !body{continue}
        for p in shape.descendants().filter(|n|n.has_tag_name(("http://schemas.openxmlformats.org/drawingml/2006/main","p"))) {
            let text:String=p.descendants().filter(|n|n.has_tag_name(("http://schemas.openxmlformats.org/drawingml/2006/main","t"))).filter_map(|n|n.text()).collect();
            if !text.trim().is_empty(){paragraphs.push(text.trim().to_string());}
        }
    }paragraphs.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_notes_follow_reordered_slides_and_decode_xml_entities() {
        use std::io::Write;
        let fixture=Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/openpager-demo.pptx");
        let mut original=ZipArchive::new(File::open(&fixture).unwrap()).unwrap();
        let temp=std::env::temp_dir().join(format!("knoend-reordered-{}.pptx",std::process::id()));
        let mut output=zip::ZipWriter::new(File::create(&temp).unwrap());
        for i in 0..original.len(){
            let mut item=original.by_index(i).unwrap();let name=item.name().to_string();let mut data=Vec::new();item.read_to_end(&mut data).unwrap();
            if name=="ppt/presentation.xml" {
                let mut xml=String::from_utf8(data).unwrap();
                let doc=roxmltree::Document::parse(&xml).unwrap();
                let ids:Vec<_>=doc.descendants().filter(|n|n.has_tag_name(("http://schemas.openxmlformats.org/presentationml/2006/main","sldId"))).collect();
                let reversed=ids.iter().rev().map(|n|&xml[n.range()]).collect::<String>();
                let range=ids.first().unwrap().range().start..ids.last().unwrap().range().end;
                xml.replace_range(range,&reversed);data=xml.into_bytes();
            }
            output.start_file(name,zip::write::SimpleFileOptions::default()).unwrap();output.write_all(&data).unwrap();
        }
        output.finish().unwrap();
        let deck=load_presentation_from_path(&temp,"reordered.pptx").unwrap();
        std::fs::remove_file(&temp).unwrap();
        assert_eq!(deck.notes[&1][0],"Point to the repository. Take questions.");
        assert!(deck.notes[&5][0].starts_with("OpenPager intro:"));
        let xml=r#"<p:notes xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:sp><p:ph type="body"/><a:p><a:r><a:t>A &amp; B &lt; C</a:t></a:r></a:p></p:sp><p:sp><p:ph type="sldNum"/><a:p><a:r><a:t>9</a:t></a:r></a:p></p:sp></p:notes>"#;
        assert_eq!(parse_notes_xml(xml),"A & B < C");
        assert_eq!(package_target("ppt/slides/slide4.xml","../notesSlides/notesSlide2.xml").as_deref(),Some("ppt/notesSlides/notesSlide2.xml"));
        assert!(package_target("ppt/slides/slide4.xml","../../../outside.xml").is_none());
    }

    #[test]
    fn live_notes_follow_actual_slide_and_preserve_only_matching_note_pages() {
        let live = LiveSlide {available:true,presenting:true,name:"Example [WIP].pptx".into(),path:String::new(),notes_available:true,slide:4,total:20,notes:"unsaved speaker notes ".repeat(35)};
        let mut previous = presentation_from_live(&live,None);
        assert_eq!(previous.current_slide,4);
        assert!(previous.notes[&4].iter().all(|p|p.len()<=220));
        previous.note_page=1;
        assert_eq!(presentation_from_live(&live,Some(&previous)).note_page,1);
        let other = LiveSlide {slide:5,notes:String::new(),..live};
        let next = presentation_from_live(&other,Some(&previous));
        assert_eq!(next.note_page,0);
        assert_eq!(next.notes[&5],vec!["No notes for this slide"]);
    }

    #[test]
    fn editing_filename_preserves_bracketed_project_suffix() {
        assert_eq!(extract_presentation_name("Quarterly review [WIP] - PowerPoint"),Some("Quarterly review [WIP]".into()));
        assert_eq!(extract_presentation_name("Quarterly review [WIP].pptx - PowerPoint"),Some("Quarterly review [WIP].pptx".into()));
    }

    #[test]
    fn extract_name_from_titles() {
        assert_eq!(
            extract_presentation_name("Example deck.pptx [Protected View] - PowerPoint (Unlicensed Product)"),
            Some("Example deck.pptx".to_string())
        );
        assert_eq!(
            extract_presentation_name("Example deck.pptx - PowerPoint (Unlicensed Product)"),
            Some("Example deck.pptx".to_string())
        );
        assert_eq!(
            extract_presentation_name("PowerPoint Slide Show - [Example deck.pptx]"),
            Some("Example deck.pptx".to_string())
        );
        assert_eq!(
            extract_presentation_name("PowerPoint Slide Show - [Example deck.pptx [Protected View]]"),
            Some("Example deck.pptx".to_string())
        );
    }

    #[test]
    fn test_is_presenting() {
        assert!(is_presenting("PowerPoint Slide Show - [Example deck.pptx]"));
        assert!(is_presenting("Diaporama PowerPoint - [Presentation1.pptx]"));
        assert!(!is_presenting("Example deck.pptx - PowerPoint"));
        assert!(!is_presenting("PowerPoint"));
    }

    /// A made-up OpenPager deck in the repository (built by `tools/make_test_pptx.py`).
    #[test]
    fn load_openpager_demo_notes() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/openpager-demo.pptx");
        let pres = load_presentation_from_path(&path, "openpager-demo.pptx").expect("deck should load");
        assert_eq!(pres.file_name, "openpager-demo.pptx");
        assert_eq!(pres.total_slides, 5);

        let note1 = pres.notes.get(&1).expect("slide 1 should have notes");
        // Exactly the notes text: the slide image and slide number shapes are left out.
        assert_eq!(
            note1,
            &vec!["OpenPager intro: hold up the board, say what it is in one line, then move on.".to_string()]
        );

        let note2 = pres.notes.get(&2).expect("slide 2 should have notes");
        assert_eq!(note2[0], "Ask who owns a macro pad.\nAsk who set it up for more than one app.");

        let note3 = pres.notes.get(&3).expect("slide 3 should have notes");
        assert!(note3.len() > 1, "slide 3 notes should be paginated (were {} pages)", note3.len());
        assert!(note3.iter().all(|p| p.len() <= 220), "a page is longer than the board takes: {note3:?}");
        assert!(note3.join(" ").contains("approve a coding agent's command"));

        // Slide 4 has no notes; slide 5's notes are stored as notesSlide4.xml, found through its rels.
        assert!(pres.notes.get(&4).is_none());
        let note5 = pres.notes.get(&5).expect("slide 5 should have notes");
        assert_eq!(note5[0], "Point to the repository. Take questions.");
    }

    #[test]
    fn test_paginate_notes() {
        let long_note = "Line 1: This is a note with some words.\nLine 2: Here is another sentence.\nLine 3: Yet more information to read while presenting.\nLine 4: Detailed talking points for the speaker.";
        let pages = paginate_notes(long_note, 80);
        assert!(pages.len() > 1);
        for p in &pages {
            assert!(p.len() <= 80, "Page too long: {}", p.len());
        }
    }

    #[test]
    fn test_unsaved_presentation_mode() {
        clear();
        let focus = Focus {
            os: knoend_core::focus::Os::Windows,
            app: "POWERPNT.EXE".into(),
            instance: "powerpnt".into(),
            title: "PowerPoint Slide Show - [Presentation1]".into(),
            web: None,
            pid: None,
            terminal: None,
        };
        let notes = get_notes(&focus);
        assert!(notes.is_some(), "Unsaved presentation should still enter presentation mode");
        let sn = notes.unwrap();
        assert!(sn.is_presenting);
        #[cfg(not(windows))]
        assert_eq!(sn.notes, "No notes for this slide");
        #[cfg(windows)]
        assert!(sn.notes.contains("unavailable"));
        assert_eq!(sn.slide_index, 1);
        assert_eq!(sn.total_slides, 0);

        // Next slide advances slide_index even without total_slides known
        let next = next_slide().unwrap();
        assert_eq!(next.slide_index, 2);
        #[cfg(target_os = "windows")]
        {

            let mut editor = focus.clone();
            editor.title = "Presentation1 - PowerPoint".into();
            assert!(get_notes(&editor).is_none());
            assert!(current_slide_notes().is_none());

        }
        clear();
    }
}

/// Windows reads the actual running slide and unsaved notes. No paths or notes
/// leave this worker's stdout/in-memory state or enter the service journal.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct LiveSlide {
    pub available: bool,
    pub presenting: bool,
    pub name: String,
    #[serde(default)] pub path: String,
    #[serde(default)] pub notes_available: bool,
    pub slide: u32,
    pub total: u32,
    pub notes: String,
}
static LIVE: Mutex<Option<(std::time::Instant, LiveSlide)>> = Mutex::new(None);
fn live_slide() -> Option<LiveSlide> {
    LIVE.lock().ok()?.as_ref().filter(|(at, _)| at.elapsed().as_secs() < 5).map(|(_, s)| s.clone())
}
fn presentation_from_live(live: &LiveSlide, previous: Option<&ActivePresentation>) -> ActivePresentation {
    let note_page = previous.filter(|a| a.file_name == live.name && a.current_slide == live.slide).map_or(0,|a|a.note_page);
    let mut notes = HashMap::new();
    notes.insert(live.slide,paginate_notes(&live.notes,220));
    ActivePresentation {file_name:live.name.clone(),total_slides:live.total,current_slide:live.slide,note_page,notes}
}
pub fn diagnostics() -> serde_json::Value {
    let live = live_slide();
    serde_json::json!({"live_reader_fresh": live.is_some(), "live_reader_available": live.as_ref().is_some_and(|s| s.available),
        "presentation_active": live.as_ref().is_some_and(|s| s.presenting), "slide":live.as_ref().map(|s|s.slide),
        "total_slides":live.as_ref().map(|s|s.total), "has_notes":live.as_ref().is_some_and(|s| !s.notes.trim().is_empty()),
        "presentation":crate::presentation::diagnostics(),"notes_source":notes_settings().source,"file_bound":!notes_settings().file.is_empty(), "pointer_supported":cfg!(windows)})
}
pub fn watch(mut changed: impl FnMut() -> bool + Send + 'static) {
    #[cfg(windows)] std::thread::spawn(move || {
        use std::io::{BufRead, BufReader};
        use std::process::{Command, Stdio};
        use std::os::windows::process::CommandExt;
        let executable=match std::env::current_exe(){Ok(p)=>p,Err(e)=>{problem!("powerpoint-reader","Could not find native worker: {e}");return;}};
        loop {
            let mut child = match Command::new(&executable).args(["__native-worker","powerpoint"])
                .env("KNOEND_PARENT_PID",std::process::id().to_string()).creation_flags(0x08000000)
                .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn() {
                Ok(child) => child, Err(e) => { problem!("powerpoint-reader", "Could not start PowerPoint reader: {e}"); return; }
            };
            let mut previous = None;
            let output = child.stdout.take().unwrap();
            use std::sync::{Arc,atomic::{AtomicBool,AtomicU64,Ordering}};
            let child = Arc::new(Mutex::new(child));
            let clock = || std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
            let progress = Arc::new(AtomicU64::new(clock()));
            let finished = Arc::new(AtomicBool::new(false));
            let (guard, tick, done) = (child.clone(),progress.clone(),finished.clone());
            std::thread::spawn(move || {
                while !done.load(Ordering::Relaxed) {
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    if clock().saturating_sub(tick.load(Ordering::Relaxed)) > 30 {
                        let _ = guard.lock().unwrap().kill(); break;
                    }
                }
            });
            let mut open = true;
            for line in BufReader::new(output).lines().map_while(Result::ok) {
                if let Ok(state) = serde_json::from_str::<LiveSlide>(&line) {
                    progress.store(clock(),Ordering::Relaxed);
                    let different = previous.as_ref() != Some(&state);
                    *LIVE.lock().unwrap() = Some((std::time::Instant::now(),state.clone()));
                    previous = Some(state);
                    if different && !changed() { open=false;break; }
                }
            }
            finished.store(true,Ordering::Relaxed);
            let _ = child.lock().unwrap().kill(); let _ = child.lock().unwrap().wait();
            *LIVE.lock().unwrap() = None;
            if !open { return; }
            problem!("powerpoint-reader", "PowerPoint reader stopped; retrying in 5 seconds");
            std::thread::sleep(std::time::Duration::from_secs(5));
        }
    });
    #[cfg(not(windows))] let _ = changed;
}

#[derive(Clone,serde::Serialize,serde::Deserialize)]
#[serde(default)]
pub struct NotesSettings {pub source:String,pub file:String}
impl Default for NotesSettings{fn default()->Self{Self{source:"recipe".into(),file:String::new()}}}
pub fn notes_settings()->NotesSettings{std::fs::read_to_string(knoend_transport::pairing::config_dir().join("presentation.json")).ok().and_then(|s|serde_json::from_str(&s).ok()).unwrap_or_default()}
pub fn configure_notes(source:Option<&str>,file:Option<&str>)->Result<(),String>{
 let mut settings=notes_settings();if let Some(source)=source{if !["recipe","live","pptx","live_or_pptx"].contains(&source){return Err("Unknown notes source".into())}settings.source=source.into();}
 if let Some(file)=file{if !file.is_empty(){let p=Path::new(file);if !p.is_absolute()||!p.is_file()||!p.extension().is_some_and(|x|x.eq_ignore_ascii_case("pptx")){return Err("Choose an existing absolute .pptx file, or clear the binding".into())}}settings.file=file.into();}
 let path=knoend_transport::pairing::config_dir().join("presentation.json");std::fs::create_dir_all(path.parent().unwrap()).map_err(|e|e.to_string())?;std::fs::write(path,serde_json::to_string_pretty(&settings).unwrap()).map_err(|e|e.to_string())
}
fn same_file(a:&Path,b:&Path)->bool{a.canonicalize().ok().zip(b.canonicalize().ok()).is_some_and(|(a,b)|a.to_string_lossy().eq_ignore_ascii_case(&b.to_string_lossy()))}
fn unavailable(name:&str,total:u32,index:u32,message:&str)->SlideNotes{let mut notes=HashMap::new();notes.insert(index,vec![message.into()]);*ACTIVE.lock().unwrap()=Some(ActivePresentation{file_name:name.into(),total_slides:total,current_slide:index,note_page:0,notes});current_slide_notes().unwrap()}
static FILE_CACHE:Mutex<Option<(PathBuf,std::time::SystemTime,u64,ActivePresentation)>>=Mutex::new(None);
fn cached_file(path:&Path,name:&str)->Option<ActivePresentation>{
 let meta=path.metadata().ok()?;let stamp=meta.modified().ok()?;let mut cache=FILE_CACHE.lock().ok()?;
 if let Some((old,modified,len,pres))=cache.as_ref(){if same_file(old,path)&&*modified==stamp&&*len==meta.len(){return Some(pres.clone())}}
 let pres=load_presentation_from_path(path,name)?;*cache=Some((path.to_path_buf(),stamp,meta.len(),pres.clone()));Some(pres)
}
