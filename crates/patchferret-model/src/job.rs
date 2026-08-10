//! Job metadata — the things a show file does not know.
//!
//! An engineer's paperwork needs the event, the act, the venue, who mixed it
//! and how to reach them. **None of that is in a console show file**, which
//! stores a mixer state and nothing about the job it was built for. So this is
//! a separate input, supplied by the user, and deliberately *not* part of
//! [`Show`](crate::Show) — that model means "what the file says", and quietly
//! mixing user-typed text into it would destroy the one property that makes it
//! trustworthy.
//!
//! A few fields overlap with things a show file *does* carry (console,
//! firmware). Those are treated as overrides: empty means "use what was
//! parsed", which is why [`JobInfo::console_or`] and friends exist rather than
//! the reports reaching for one source or the other ad hoc.

/// An image supplied for the report header.
#[derive(Debug, Clone, PartialEq)]
pub struct Logo {
    /// Encoded image bytes, as supplied.
    pub bytes: Vec<u8>,
}

impl Logo {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }
}

/// Everything the user tells us about the job.
///
/// Every field is optional; a report with an empty `JobInfo` looks exactly as
/// it did before this existed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct JobInfo {
    /// Event or production name — the headline on the cover.
    pub event: Option<String>,
    /// Date, free text. Deliberately not a date type: "12–14 Sept", "Sat only"
    /// and "get-in Thu" are all things people legitimately write here, and
    /// forcing a calendar date would make the field useless for half of them.
    pub date: Option<String>,
    /// Artist, act or client.
    pub artist: Option<String>,
    pub venue: Option<String>,
    pub production_company: Option<String>,
    pub engineer: Option<String>,
    /// Phone, email, or both — free text.
    pub engineer_contact: Option<String>,
    /// Overrides the console named in the show file.
    pub console: Option<String>,
    /// Overrides the firmware/format version from the show file.
    pub firmware: Option<String>,
    /// Free note printed under the header grid.
    pub notes: Option<String>,
    /// Arbitrary extra label/value pairs, shown after the known fields.
    pub custom: Vec<(String, String)>,
    pub logo: Option<Logo>,
}

fn clean(v: &str) -> Option<String> {
    let t = v.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

impl JobInfo {
    pub fn is_empty(&self) -> bool {
        self.event.is_none()
            && self.date.is_none()
            && self.artist.is_none()
            && self.venue.is_none()
            && self.production_company.is_none()
            && self.engineer.is_none()
            && self.engineer_contact.is_none()
            && self.console.is_none()
            && self.firmware.is_none()
            && self.notes.is_none()
            && self.custom.is_empty()
            && self.logo.is_none()
    }

    /// The console to print: the user's override, else what was parsed.
    pub fn console_or<'a>(&'a self, parsed: &'a str) -> &'a str {
        self.console.as_deref().unwrap_or(parsed)
    }

    /// The firmware/version to print: the user's override, else what was parsed.
    pub fn firmware_or<'a>(&'a self, parsed: Option<&'a str>) -> Option<&'a str> {
        self.firmware.as_deref().or(parsed)
    }

    /// Known label/value pairs that are set, in print order.
    ///
    /// Console and firmware are not here — they need the parsed show to fall
    /// back to, so the report adds them.
    pub fn fields(&self) -> Vec<(&'static str, &str)> {
        [
            ("Date", &self.date),
            ("Artist / act", &self.artist),
            ("Venue", &self.venue),
            ("Production", &self.production_company),
            ("Engineer", &self.engineer),
            ("Contact", &self.engineer_contact),
        ]
        .into_iter()
        .filter_map(|(label, value)| value.as_deref().map(|v| (label, v)))
        .collect()
    }

    /// Set a field by its sidecar key. Unknown keys become custom fields.
    ///
    /// Returns false only for a key that is reserved but unusable here (the
    /// logo, which is bytes rather than text).
    pub fn set(&mut self, key: &str, value: &str) -> bool {
        let v = clean(value);
        match key.trim().to_ascii_lowercase().replace([' ', '-'], "_").as_str() {
            "event" | "show" | "production_name" => self.event = v,
            "date" | "dates" => self.date = v,
            "artist" | "act" | "client" | "band" => self.artist = v,
            "venue" => self.venue = v,
            "production" | "production_company" | "company" => self.production_company = v,
            "engineer" | "sound_engineer" | "foh" | "operator" => self.engineer = v,
            "contact" | "engineer_contact" | "phone" | "email" => self.engineer_contact = v,
            "console" | "desk" => self.console = v,
            "firmware" | "firmware_version" | "version" => self.firmware = v,
            "notes" | "note" => self.notes = v,
            "logo" => return false,
            other => {
                if let Some(v) = v {
                    // Preserve the user's own capitalisation for the label.
                    let label = key.trim().to_string();
                    let _ = other;
                    if let Some(slot) = self.custom.iter_mut().find(|(k, _)| k == &label) {
                        slot.1 = v;
                    } else {
                        self.custom.push((label, v));
                    }
                }
            }
        }
        true
    }

    /// Parse a dependency-free `key: value` sidecar.
    ///
    /// Blank lines and `#` comments are ignored. Unknown keys become custom
    /// fields, so an engineer can add "Rider rev" or "Truck call" without the
    /// tool needing to know about them. `logo:` names a file and is returned
    /// separately, because loading it is the caller's business.
    pub fn parse_sidecar(text: &str) -> (JobInfo, Option<String>) {
        let mut job = JobInfo::default();
        let mut logo_path = None;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            if key.trim().eq_ignore_ascii_case("logo") {
                logo_path = clean(value);
                continue;
            }
            job.set(key, value);
        }
        (job, logo_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_by_default() {
        assert!(JobInfo::default().is_empty());
    }

    #[test]
    fn parses_a_sidecar() {
        let (job, logo) = JobInfo::parse_sidecar(
            "# a comment\n\
             Event: Summer Live 2026\n\
             Date: 12-14 Sept\n\
             Artist: The Something Band\n\
             Venue: Old Granada Studios\n\
             Production: Stoatworks\n\
             Engineer: A. Sargeant\n\
             Contact: a@example.com\n\
             logo: /tmp/logo.jpg\n",
        );
        assert_eq!(job.event.as_deref(), Some("Summer Live 2026"));
        assert_eq!(job.date.as_deref(), Some("12-14 Sept"));
        assert_eq!(job.artist.as_deref(), Some("The Something Band"));
        assert_eq!(job.engineer_contact.as_deref(), Some("a@example.com"));
        assert_eq!(logo.as_deref(), Some("/tmp/logo.jpg"));
        assert!(!job.is_empty());
    }

    #[test]
    fn unknown_keys_become_custom_fields() {
        let (job, _) = JobInfo::parse_sidecar("Truck call: 0600\nRider rev: C\n");
        assert_eq!(
            job.custom,
            vec![
                ("Truck call".to_string(), "0600".to_string()),
                ("Rider rev".to_string(), "C".to_string())
            ]
        );
    }

    #[test]
    fn key_spelling_is_forgiving() {
        for key in ["Engineer", "sound engineer", "FOH", "operator", "SOUND-ENGINEER"] {
            let (job, _) = JobInfo::parse_sidecar(&format!("{key}: Sam\n"));
            assert_eq!(job.engineer.as_deref(), Some("Sam"), "key {key:?} not recognised");
        }
    }

    #[test]
    fn a_value_containing_a_colon_survives() {
        // Times and URLs both contain colons; splitting on the last one would
        // mangle them.
        let (job, _) = JobInfo::parse_sidecar(
            "Contact: 07700 900000, sam@example.com\nDate: 19:30 doors\n",
        );
        assert_eq!(job.date.as_deref(), Some("19:30 doors"));
        assert!(job.engineer_contact.as_deref().unwrap().contains("@"));
    }

    #[test]
    fn blank_values_clear_rather_than_printing_empty_rows() {
        let (job, _) = JobInfo::parse_sidecar("Event:   \nVenue: Hall\n");
        assert_eq!(job.event, None);
        assert_eq!(job.venue.as_deref(), Some("Hall"));
        assert_eq!(job.fields().len(), 1);
    }

    #[test]
    fn overrides_fall_back_to_the_parsed_show() {
        let mut job = JobInfo::default();
        assert_eq!(job.console_or("Yamaha DM3"), "Yamaha DM3");
        assert_eq!(job.firmware_or(Some("52020007")), Some("52020007"));
        job.console = Some("DM3 (spare)".into());
        assert_eq!(job.console_or("Yamaha DM3"), "DM3 (spare)");
    }

    #[test]
    fn malformed_lines_are_skipped_not_fatal() {
        let (job, _) = JobInfo::parse_sidecar("no colon here\n\n#comment\nVenue: Hall\n");
        assert_eq!(job.venue.as_deref(), Some("Hall"));
    }
}
