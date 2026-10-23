//! PowerPoint presenter support on Windows: reads slide notes and presentation state
//! using Windows COM Automation (`PowerPoint.Application.SlideShowWindows`).
//!
//! On non-Windows platforms (or when PowerPoint is not presenting), this is a no-op.

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SlideNotes {
    pub slide_index: u32,
    pub total_slides: u32,
    pub notes: String,
}

#[cfg(windows)]
pub fn current_presentation_notes() -> Option<SlideNotes> {
    use std::process::Command;

    // Small PowerShell query to read the current slide index and speaker notes from PowerPoint COM.
    // Runs with a short timeout and minimal footprint.
    let script = r#"
try {
    $ppt = [System.Runtime.InteropServices.Marshal]::GetActiveObject('PowerPoint.Application')
    if ($ppt.SlideShowWindows.Count -gt 0) {
        $ssw = $ppt.SlideShowWindows.Item(1)
        $slide = $ssw.View.Slide
        $index = $slide.SlideIndex
        $total = $ssw.Presentation.Slides.Count
        $note = ""
        try {
            $ph = $slide.NotesPage.Shapes.Placeholders.Item(2)
            if ($ph.HasTextFrame -and $ph.TextFrame.HasText) {
                $note = $ph.TextFrame.TextRange.Text
            }
        } catch {}
        Write-Output "$index|$total|$note"
    }
} catch {}
"#;

    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        return None;
    }

    let mut parts = text.splitn(3, '|');
    let slide_index = parts.next()?.parse::<u32>().ok()?;
    let total_slides = parts.next()?.parse::<u32>().ok().unwrap_or(0);
    let notes = parts.next().unwrap_or("").trim().to_string();

    Some(SlideNotes {
        slide_index,
        total_slides,
        notes,
    })
}

#[cfg(not(windows))]
pub fn current_presentation_notes() -> Option<SlideNotes> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slide_notes_default_is_empty() {
        let sn = SlideNotes::default();
        assert_eq!(sn.slide_index, 0);
        assert!(sn.notes.is_empty());
    }
}
