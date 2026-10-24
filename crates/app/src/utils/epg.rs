//! XMLTV export of the channel schedule (VizRS EPG).
//!
//! The programme list is built from the daily playlists through
//! [`build_program`]; this module only turns it into an XMLTV document
//! (<https://github.com/XMLTV/xmltv/blob/master/xmltv.dtd>).

use std::path::Path;

use chrono::{DateTime, FixedOffset, NaiveTime, TimeDelta};

use crate::utils::{
    config::PlayoutConfig,
    errors::ServiceError,
    program::{ProgramItem, ProgramObj, build_program},
    time_machine::time_now,
};

/// Default and maximum number of days exported to the guide.
pub const EPG_DEFAULT_DAYS: i64 = 3;
pub const EPG_MAX_DAYS: i64 = 14;

/// One `<programme>` entry of the guide.
#[derive(Debug, Clone, PartialEq)]
pub struct EpgProgramme {
    pub start: DateTime<FixedOffset>,
    pub stop: DateTime<FixedOffset>,
    pub title: String,
}

/// Channel header of the guide.
#[derive(Debug, Clone)]
pub struct EpgChannel {
    pub id: String,
    pub display_name: String,
}

/// Collect the guide entries for `days` days, starting today in the channel timezone.
/// Commercial breaks (`ad`) are left out.
pub async fn build_epg(
    config: &PlayoutConfig,
    days: i64,
) -> Result<Vec<EpgProgramme>, ServiceError> {
    let days = days.clamp(1, EPG_MAX_DAYS);
    let today = time_now(&config.channel.timezone).date_naive();
    let last_day = today + TimeDelta::try_days(days - 1).unwrap_or_default();
    let end_of_day = NaiveTime::from_hms_opt(23, 59, 59)
        .ok_or_else(|| ServiceError::Conflict("Invalid end-of-day time".to_string()))?;
    let obj = ProgramObj {
        start_after: Some(today.and_time(NaiveTime::MIN)),
        start_before: Some(last_day.and_time(end_of_day)),
    };

    build_program(config, obj)
        .await?
        .into_iter()
        .filter(|item| !item.ad)
        .map(programme_from_item)
        .collect()
}

fn programme_from_item(item: ProgramItem) -> Result<EpgProgramme, ServiceError> {
    let start = DateTime::parse_from_str(&item.start, "%Y-%m-%d %H:%M:%S%.3f%:z")
        .map_err(|e| ServiceError::Conflict(format!("Invalid programme start: {e}")))?;
    let length =
        TimeDelta::try_milliseconds(((item.out - item.r#in) * 1000.0) as i64).unwrap_or_default();
    let title = item
        .title
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| title_from_source(&item.source));

    Ok(EpgProgramme {
        start,
        stop: start + length,
        title,
    })
}

/// Fallback title: file name without extension, `_` and `.` turned into spaces.
fn title_from_source(source: &str) -> String {
    Path::new(source)
        .file_stem()
        .map(|stem| stem.to_string_lossy().replace(['_', '.'], " "))
        .unwrap_or_else(|| source.to_string())
}

fn xml_escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());

    for c in text.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => escaped.push(c),
        }
    }

    escaped
}

fn xmltv_time(time: &DateTime<FixedOffset>) -> String {
    time.format("%Y%m%d%H%M%S %z").to_string()
}

/// Render the XMLTV document.
pub fn to_xmltv(channel: &EpgChannel, programmes: &[EpgProgramme]) -> String {
    let id = xml_escape(&channel.id);
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE tv SYSTEM \"xmltv.dtd\">\n<tv generator-info-name=\"VizRS Playout\">\n",
    );

    xml.push_str(&format!(
        "  <channel id=\"{id}\">\n    <display-name>{}</display-name>\n  </channel>\n",
        xml_escape(&channel.display_name)
    ));

    for programme in programmes {
        xml.push_str(&format!(
            "  <programme start=\"{}\" stop=\"{}\" channel=\"{id}\">\n    <title>{}</title>\n  </programme>\n",
            xmltv_time(&programme.start),
            xmltv_time(&programme.stop),
            xml_escape(&programme.title)
        ));
    }

    xml.push_str("</tv>\n");

    xml
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(start: &str, title: Option<&str>, ad: bool) -> ProgramItem {
        ProgramItem {
            source: "/media/Morning_News.part1.mp4".to_string(),
            start: start.to_string(),
            title: title.map(str::to_string),
            r#in: 10.0,
            out: 70.5,
            duration: 90.0,
            ad,
        }
    }

    #[test]
    fn programme_uses_cut_length_and_title_fallback() {
        let programme =
            programme_from_item(item("2026-07-19 12:00:00.000+02:00", None, false)).unwrap();

        assert_eq!(programme.title, "Morning News part1");
        assert_eq!(xmltv_time(&programme.start), "20260719120000 +0200");
        assert_eq!(xmltv_time(&programme.stop), "20260719120100 +0200");
    }

    #[test]
    fn xmltv_escapes_text_and_lists_programmes() {
        let channel = EpgChannel {
            id: "vizrs.1".to_string(),
            display_name: "News & <Sport>".to_string(),
        };
        let programme = programme_from_item(item(
            "2026-07-19 12:00:00.000+02:00",
            Some("\"A\" & B"),
            false,
        ))
        .unwrap();

        let xml = to_xmltv(&channel, &[programme]);

        assert!(xml.contains("<display-name>News &amp; &lt;Sport&gt;</display-name>"));
        assert!(xml.contains(
            "<programme start=\"20260719120000 +0200\" stop=\"20260719120100 +0200\" channel=\"vizrs.1\">"
        ));
        assert!(xml.contains("<title>&quot;A&quot; &amp; B</title>"));
        assert!(xml.ends_with("</tv>\n"));
    }

    #[test]
    fn xml_escape_drops_invalid_control_characters() {
        assert_eq!(xml_escape("a\u{1}b\tc"), "ab\tc");
    }
}
