//! In-memory PowerPoint presenter notes in pure Rust.
//! Reads speaker notes directly from the presentation (.pptx) archive into memory.
//! No COM, no PowerShell, cross-platform and instant (zero lag).

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

#[derive(Debug, Default)]
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
    is_presenting(title) || MANUAL_PRESENTING.lock().map_or(false, |p| *p)
}

/// Called when PowerPoint has focus: returns slide notes ONLY if PowerPoint is in presentation mode.
pub fn get_notes(focus: &Focus) -> Option<SlideNotes> {
    let title = &focus.title;
    if !is_active_presenting(title) {
        return None;
    }

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
        let start = candidate.rfind('[').map(|i| i + 1)
            .or_else(|| candidate.rfind(" - ").map(|i| i + 3))
            .unwrap_or(0);
        let name = candidate[start..].trim().trim_matches('[').trim_matches(']');
        if !name.is_empty() {
            return Some(name.to_string());
        }
    }

    // Try strip " - PowerPoint" suffix if no .pptx extension is displayed in title
    if let Some(pos) = lower.find(" - powerpoint") {
        let candidate = &title[..pos];
        let start = candidate.rfind('[').map(|i| i + 1)
            .or_else(|| candidate.rfind(" - ").map(|i| i + 3))
            .unwrap_or(0);
        let name = candidate[start..].trim().trim_matches('[').trim_matches(']');
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

    let mut slide_entries = Vec::new();
    for i in 0..archive.len() {
        if let Ok(file) = archive.by_index(i) {
            let name = file.name();
            if name.starts_with("ppt/slides/slide") && name.ends_with(".xml") {
                slide_entries.push(name.to_string());
            }
        }
    }

    let mut slide_nums: Vec<u32> = slide_entries
        .iter()
        .filter_map(|name| {
            let num_str = name.strip_prefix("ppt/slides/slide")?.strip_suffix(".xml")?;
            num_str.parse::<u32>().ok()
        })
        .collect();
    slide_nums.sort_unstable();

    let total_slides = slide_nums.len() as u32;

    let mut notes = HashMap::new();
    for (idx, slide_num) in slide_nums.iter().enumerate() {
        let slide_index_1_based = (idx + 1) as u32;
        let rel_path = format!("ppt/slides/_rels/slide{slide_num}.xml.rels");
        let mut note_entry_name = None;

        if let Ok(mut rel_file) = archive.by_name(&rel_path) {
            let mut rel_xml = String::new();
            if rel_file.read_to_string(&mut rel_xml).is_ok() {
                if let Some(target) = extract_rel_target(&rel_xml, "notesSlide") {
                    let cleaned = target.trim_start_matches("../");
                    note_entry_name = Some(format!("ppt/{cleaned}"));
                }
            }
        }

        let entry_to_read = note_entry_name.unwrap_or_else(|| {
            format!("ppt/notesSlides/notesSlide{slide_num}.xml")
        });

        if let Ok(mut note_file) = archive.by_name(&entry_to_read) {
            let mut xml = String::new();
            if note_file.read_to_string(&mut xml).is_ok() {
                let raw_text = parse_notes_xml(&xml);
                let pages = paginate_notes(&raw_text, 220);
                notes.insert(slide_index_1_based, pages);
            }
        }
    }

    Some(ActivePresentation {
        file_name: file_name.to_string(),
        total_slides,
        current_slide: 1,
        note_page: 0,
        notes,
    })
}

fn extract_rel_target(xml: &str, target_type: &str) -> Option<String> {
    for rel in xml.split("<Relationship ") {
        if rel.contains(target_type) {
            if let Some(target_start) = rel.find("Target=\"") {
                let rest = &rel[target_start + 8..];
                if let Some(target_end) = rest.find('"') {
                    return Some(rest[..target_end].to_string());
                }
            }
        }
    }
    None
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

fn parse_notes_xml(xml: &str) -> String {
    let mut paragraphs = Vec::new();

    let mut rest_sp = xml;
    while let Some(sp_start) = rest_sp.find("<p:sp>") {
        let after_sp = &rest_sp[sp_start + 6..];
        let Some(sp_end) = after_sp.find("</p:sp>") else { break; };
        let sp_content = &after_sp[..sp_end];

        let is_meta_shape = sp_content.contains("type=\"sldNum\"")
            || sp_content.contains("type=\"dt\"")
            || sp_content.contains("type=\"hdr\"")
            || sp_content.contains("type=\"sldImg\"");

        if !is_meta_shape {
            let mut rest_p = sp_content;
            while let Some(p_start) = rest_p.find("<a:p>") {
                let after_p = &rest_p[p_start + 5..];
                let Some(p_end) = after_p.find("</a:p>") else { break; };
                let p_content = &after_p[..p_end];

                let mut p_text = String::new();
                let mut p_rest = p_content;
                while let Some(t_start) = p_rest.find("<a:t") {
                    let after_t = &p_rest[t_start..];
                    let Some(tag_end) = after_t.find('>') else { break; };
                    let t_content = &after_t[tag_end + 1..];
                    let Some(t_close) = t_content.find("</a:t>") else { break; };
                    p_text.push_str(&t_content[..t_close]);
                    p_rest = &t_content[t_close + 6..];
                }
                let trimmed = p_text.trim();
                if !trimmed.is_empty() {
                    paragraphs.push(trimmed.to_string());
                }
                rest_p = &after_p[p_end + 6..];
            }
        }

        rest_sp = &after_sp[sp_end + 7..];
    }

    if paragraphs.is_empty() {
        let mut rest = xml;
        while let Some(p_start) = rest.find("<a:p>") {
            let after_p = &rest[p_start + 5..];
            let Some(p_end) = after_p.find("</a:p>") else { break; };
            let p_content = &after_p[..p_end];

            let mut p_text = String::new();
            let mut p_rest = p_content;
            while let Some(t_start) = p_rest.find("<a:t") {
                let after_t = &p_rest[t_start..];
                let Some(tag_end) = after_t.find('>') else { break; };
                let t_content = &after_t[tag_end + 1..];
                let Some(t_close) = t_content.find("</a:t>") else { break; };
                p_text.push_str(&t_content[..t_close]);
                p_rest = &t_content[t_close + 6..];
            }
            let trimmed = p_text.trim();
            if !trimmed.is_empty() && trimmed.parse::<u32>().is_err() {
                paragraphs.push(trimmed.to_string());
            }
            rest = &after_p[p_end + 6..];
        }
    }

    paragraphs.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_name_from_titles() {
        assert_eq!(
            extract_presentation_name("Copy of MindStudio Bootcamp.pptx [Protected View] - PowerPoint (Unlicensed Product)"),
            Some("Copy of MindStudio Bootcamp.pptx".to_string())
        );
        assert_eq!(
            extract_presentation_name("Copy of MindStudio Bootcamp.pptx - PowerPoint (Unlicensed Product)"),
            Some("Copy of MindStudio Bootcamp.pptx".to_string())
        );
        assert_eq!(
            extract_presentation_name("PowerPoint Slide Show - [Copy of MindStudio Bootcamp.pptx]"),
            Some("Copy of MindStudio Bootcamp.pptx".to_string())
        );
        assert_eq!(
            extract_presentation_name("PowerPoint Slide Show - [Copy of MindStudio Bootcamp.pptx [Protected View]]"),
            Some("Copy of MindStudio Bootcamp.pptx".to_string())
        );
    }

    #[test]
    fn test_is_presenting() {
        assert!(is_presenting("PowerPoint Slide Show - [Copy of MindStudio Bootcamp.pptx]"));
        assert!(is_presenting("Diaporama PowerPoint - [Presentation1.pptx]"));
        assert!(!is_presenting("Copy of MindStudio Bootcamp.pptx - PowerPoint"));
        assert!(!is_presenting("PowerPoint"));
    }

    #[test]
    #[ignore = "needs Sundar's own Copy of MindStudio Bootcamp.pptx, not in the repository"]
    fn load_mindstudio_notes() {
        let pres = load_presentation("Copy of MindStudio Bootcamp.pptx");
        assert!(pres.is_some(), "Presentation file not found");
        let pres = pres.unwrap();
        assert_eq!(pres.total_slides, 7);
        let note1 = pres.notes.get(&1).expect("Slide 1 should have notes");
        assert!(note1[0].contains("Mind Studio intro"), "Note text was: {:?}", note1);
        let note3 = pres.notes.get(&3).expect("Slide 3 should have notes");
        assert!(note3.len() > 1, "Slide 3 notes should be paginated (was {} pages)", note3.len());
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
        assert_eq!(sn.notes, "No notes for this slide");
        assert_eq!(sn.slide_index, 1);
        assert_eq!(sn.total_slides, 0);

        // Next slide advances slide_index even without total_slides known
        let next = next_slide().unwrap();
        assert_eq!(next.slide_index, 2);
        clear();
    }
}
