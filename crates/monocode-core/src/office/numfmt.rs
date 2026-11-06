//! Excel number formats as a sheet prints them, ported from the Tauri
//! viewer's `spreadsheet.ts`: decimals, thousands, percent, currency and
//! literal units, negative and zero sections, and the date tokens. Anything
//! stranger falls back to the plain number.
use std::sync::LazyLock;

use regex::Regex;

/// The separators and names a locale prints with. The viewer used the
/// browser's `Intl`; this carries the two locales the app ships in.
#[derive(Debug, Clone)]
pub struct Locale {
    pub decimal: char,
    pub group: char,
    /// Digits a number needs before it is grouped (Spanish skips 4-digit numbers).
    min_group_digits: usize,
    day_first: bool,
    hour12: bool,
    months: [&'static str; 12],
    months_short: [&'static str; 12],
    /// Sunday first.
    weekdays: [&'static str; 7],
    weekdays_short: [&'static str; 7],
}

impl Default for Locale {
    fn default() -> Self {
        Locale::en()
    }
}

impl Locale {
    pub fn en() -> Locale {
        Locale {
            decimal: '.',
            group: ',',
            min_group_digits: 4,
            day_first: false,
            hour12: true,
            months: [
                "January",
                "February",
                "March",
                "April",
                "May",
                "June",
                "July",
                "August",
                "September",
                "October",
                "November",
                "December",
            ],
            months_short: [
                "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
            ],
            weekdays: [
                "Sunday",
                "Monday",
                "Tuesday",
                "Wednesday",
                "Thursday",
                "Friday",
                "Saturday",
            ],
            weekdays_short: ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"],
        }
    }

    pub fn es() -> Locale {
        Locale {
            decimal: ',',
            group: '.',
            min_group_digits: 5,
            day_first: true,
            hour12: false,
            months: [
                "enero",
                "febrero",
                "marzo",
                "abril",
                "mayo",
                "junio",
                "julio",
                "agosto",
                "septiembre",
                "octubre",
                "noviembre",
                "diciembre",
            ],
            months_short: [
                "ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sept", "oct", "nov", "dic",
            ],
            weekdays: [
                "domingo",
                "lunes",
                "martes",
                "miércoles",
                "jueves",
                "viernes",
                "sábado",
            ],
            weekdays_short: ["dom", "lun", "mar", "mié", "jue", "vie", "sáb"],
        }
    }

    /// The locale for a BCP 47 tag such as `es-PE` or `en-US`; English otherwise.
    pub fn from_tag(tag: &str) -> Locale {
        if tag.to_ascii_lowercase().starts_with("es") {
            Locale::es()
        } else {
            Locale::en()
        }
    }
}

static DATE_STRIP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#""[^"]*"|\\.|\[[^\]]*\]"#).expect("a valid pattern"));
static BRACKETS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[[^\]]*\]").expect("a valid pattern"));
static DATE_TOKENS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)"[^"]*"|\\.|yyyy|yy|mmmmm|mmmm|mmm|mm|m|dddd|ddd|dd|d|hh|h|ss|s|am/pm|a/p|."#)
        .expect("a valid pattern")
});
static TWELVE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)am/pm|a/p").expect("a valid pattern"));

// ---------------------------------------------------------------- sections

/// Split a format at its unquoted, unescaped semicolons.
fn split_sections(format: &str) -> Vec<String> {
    let chars: Vec<char> = format.chars().collect();
    let mut sections = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        if ch == '"' {
            quoted = !quoted;
        }
        if ch == '\\' && !quoted {
            current.push(ch);
            if let Some(next) = chars.get(index + 1) {
                current.push(*next);
            }
            index += 2;
            continue;
        }
        if ch == ';' && !quoted {
            sections.push(std::mem::take(&mut current));
        } else {
            current.push(ch);
        }
        index += 1;
    }
    sections.push(current);
    sections
}

/// The section of a format that applies, and whether it already shows the sign.
fn format_section(format: &str, value: f64) -> (String, bool) {
    let mut sections = split_sections(format);
    if value < 0.0 && sections.len() > 1 {
        return (sections.swap_remove(1), true);
    }
    if value == 0.0 && sections.len() > 2 {
        return (sections.swap_remove(2), false);
    }
    (sections.swap_remove(0), false)
}

/// The literal text around a number's digits: quoted strings, escapes,
/// currency. Returns `(before, after, body)`.
fn literals(section: &str) -> (String, String, String) {
    let chars: Vec<char> = section.chars().collect();
    let (mut before, mut after, mut body) = (String::new(), String::new(), String::new());
    let mut seen_digits = false;
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        let mut literal = String::new();
        if ch == '"' {
            let end = chars[index + 1..]
                .iter()
                .position(|c| *c == '"')
                .map(|at| at + index + 1);
            literal = chars[index + 1..end.unwrap_or(chars.len())]
                .iter()
                .collect();
            index = end.unwrap_or(chars.len());
        } else if ch == '\\' {
            if let Some(next) = chars.get(index + 1) {
                literal.push(*next);
            }
            index += 1;
        } else if ch == '[' {
            let end = chars[index..]
                .iter()
                .position(|c| *c == ']')
                .map(|at| at + index);
            let inside: String = chars[index + 1..end.unwrap_or(chars.len())]
                .iter()
                .collect();
            // `[$S/-280A]` carries a currency symbol; colours and conditions don't print.
            if let Some(currency) = inside.strip_prefix('$') {
                literal = currency.split('-').next().unwrap_or("").to_string();
            }
            index = end.unwrap_or(chars.len());
        } else if ch == '_' || ch == '*' {
            // Padding to the width of the next character.
            index += 1;
            if ch == '_' {
                literal.push(' ');
            }
        } else if "0#?.,%".contains(ch) {
            seen_digits = true;
            body.push(ch);
            index += 1;
            continue;
        } else if ch == 'E' || ch == 'e' {
            body.push(ch);
            index += 1;
            continue;
        } else if ch != '@' {
            literal.push(ch);
        }
        if seen_digits {
            after.push_str(&literal);
        } else {
            before.push_str(&literal);
        }
        index += 1;
    }
    (before, after, body)
}

pub fn is_date_format(format: Option<&str>) -> bool {
    let Some(format) = format else {
        return false;
    };
    if format.is_empty() {
        return false;
    }
    let plain = DATE_STRIP.replace_all(format, "");
    let has_date_letter = plain.chars().any(|c| "ymdhsYMDHS".contains(c));
    let leftover: String = plain
        .chars()
        .filter(|c| !"ymdhsYMDHS".contains(*c))
        .collect();
    has_date_letter && !leftover.chars().any(|c| "0#?".contains(c))
}

// ---------------------------------------------------------------- numbers

/// A finite number as JavaScript's `String(value)` prints it.
pub fn js_number_string(value: f64) -> String {
    if value.is_nan() {
        return "NaN".into();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    if value == 0.0 {
        return "0".into();
    }
    let magnitude = value.abs();
    if (1e-6..1e21).contains(&magnitude) {
        return format!("{value}");
    }
    let text = format!("{value:e}");
    match text.split_once('e') {
        Some((mantissa, exponent)) if exponent.starts_with('-') => {
            format!("{mantissa}e{exponent}")
        }
        Some((mantissa, exponent)) => format!("{mantissa}e+{exponent}"),
        None => text,
    }
}

/// The shortest decimal digits of a positive finite number and where its
/// point falls: `1234.5` is `("12345", 4)`, `0.05` is `("5", -1)`.
fn decimal_digits(value: f64) -> (Vec<u8>, i32) {
    let text = format!("{value:e}");
    let (mantissa, exponent) = text.split_once('e').unwrap_or((&text, "0"));
    let digits: Vec<u8> = mantissa
        .bytes()
        .filter(u8::is_ascii_digit)
        .map(|digit| digit - b'0')
        .collect();
    (digits, exponent.parse::<i32>().unwrap_or(0) + 1)
}

/// `value` with between `minimum` and `maximum` fraction digits, rounded half
/// away from zero on its shortest decimal form, the way `Intl.NumberFormat` does.
fn format_fixed(
    value: f64,
    minimum: usize,
    maximum: usize,
    grouping: bool,
    locale: &Locale,
) -> String {
    let negative = value < 0.0;
    let (digits, point) = decimal_digits(value.abs());
    let (mut whole, mut fraction): (Vec<u8>, Vec<u8>) = if point <= 0 {
        (
            vec![0],
            std::iter::repeat_n(0, point.unsigned_abs() as usize)
                .chain(digits)
                .collect(),
        )
    } else if point as usize >= digits.len() {
        let padding = point as usize - digits.len();
        (
            digits
                .into_iter()
                .chain(std::iter::repeat_n(0, padding))
                .collect(),
            Vec::new(),
        )
    } else {
        let (whole, fraction) = digits.split_at(point as usize);
        (whole.to_vec(), fraction.to_vec())
    };
    if fraction.len() > maximum {
        let round_up = fraction[maximum] >= 5;
        fraction.truncate(maximum);
        if round_up {
            let mut combined: Vec<u8> = whole.iter().chain(fraction.iter()).copied().collect();
            let mut at = combined.len();
            loop {
                if at == 0 {
                    combined.insert(0, 1);
                    break;
                }
                at -= 1;
                if combined[at] == 9 {
                    combined[at] = 0;
                } else {
                    combined[at] += 1;
                    break;
                }
            }
            let split = combined.len() - fraction.len();
            fraction = combined.split_off(split);
            whole = combined;
        }
    }
    while fraction.len() > minimum && fraction.last() == Some(&0) {
        fraction.pop();
    }
    while fraction.len() < minimum {
        fraction.push(0);
    }
    let whole_text: String = whole.iter().map(|d| char::from(b'0' + d)).collect();
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    if grouping && whole_text.len() >= locale.min_group_digits {
        for (at, digit) in whole_text.chars().enumerate() {
            if at > 0 && (whole_text.len() - at).is_multiple_of(3) {
                out.push(locale.group);
            }
            out.push(digit);
        }
    } else {
        out.push_str(&whole_text);
    }
    if !fraction.is_empty() {
        out.push(locale.decimal);
        out.extend(fraction.iter().map(|d| char::from(b'0' + d)));
    }
    out
}

fn general_number(value: f64, locale: &Locale) -> String {
    let magnitude = value.abs();
    if value != 0.0 && !(1e-9..1e11).contains(&magnitude) {
        let text = format!("{value:.5e}");
        let (mantissa, exponent) = text.split_once('e').unwrap_or((&text, "0"));
        let mantissa = mantissa
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string();
        let sign = if exponent.starts_with('-') { "" } else { "+" };
        return format!("{mantissa}E{sign}{exponent}");
    }
    // Eleven significant digits, as `toPrecision(11)` leaves them.
    let rounded: f64 = format!("{value:.10e}").parse().unwrap_or(value);
    format_fixed(rounded, 0, 10, false, locale)
}

/// A number as its format prints it. `None` or "General" prints it plain.
pub fn format_number(value: f64, format: Option<&str>, locale: &Locale) -> String {
    if !value.is_finite() {
        return js_number_string(value);
    }
    let value = if value == 0.0 { 0.0 } else { value };
    let Some(format) = format.filter(|f| !f.trim().eq_ignore_ascii_case("general")) else {
        return general_number(value, locale);
    };
    let (section, signed) = format_section(format, value);
    let (before, after, body) = literals(&section);
    // A section of text alone, like `"-"` for zero, prints just that text.
    if body.is_empty() && (!before.is_empty() || !after.is_empty()) {
        return format!("{before}{after}");
    }
    if body.is_empty() || body.contains(['e', 'E']) {
        return general_number(value, locale);
    }
    let percent = body.contains('%');
    let body = body.replace('%', "");
    let mut parts = body.split('.');
    let integer_part = parts.next().unwrap_or("");
    let fraction = parts.next().unwrap_or("");
    let placeholder = |c: char| "0#?".contains(c);
    let decimals = fraction.chars().filter(|c| placeholder(*c)).count();
    let minimum = fraction.chars().filter(|c| *c == '0').count();
    let grouped = integer_part
        .as_bytes()
        .windows(3)
        .any(|w| placeholder(w[0] as char) && w[1] == b',' && placeholder(w[2] as char));
    // Trailing commas scale by thousands.
    let scale = 1000f64.powi(integer_part.chars().rev().take_while(|c| *c == ',').count() as i32);
    let base = if signed { value.abs() } else { value };
    let scaled = base * if percent { 100.0 } else { 1.0 } / scale;
    let digits = format_fixed(scaled, minimum, decimals, grouped, locale);
    format!("{before}{digits}{}{after}", if percent { "%" } else { "" })
}

// ---------------------------------------------------------------- dates

/// A calendar moment without a time zone: Excel dates have none, so they are
/// read as UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateTime {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    /// 0 is Sunday.
    pub weekday: u32,
}

impl DateTime {
    pub fn new(year: i64, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> DateTime {
        let days = days_from_civil(year, month, day);
        DateTime {
            year,
            month,
            day,
            hour,
            minute,
            second,
            weekday: (days + 4).rem_euclid(7) as u32,
        }
    }

    /// An Excel serial number: days since 1899-12-30 (1904-01-01 when
    /// `date1904`), rounded to the millisecond as ExcelJS does.
    pub fn from_serial(serial: f64, date1904: bool) -> Option<DateTime> {
        let millis =
            ((serial - 25569.0 + if date1904 { 1462.0 } else { 0.0 }) * 86_400_000.0).round();
        if !millis.is_finite() || millis.abs() > 8.64e15 {
            return None;
        }
        let millis = millis as i64;
        let days = millis.div_euclid(86_400_000);
        let rest = millis.rem_euclid(86_400_000) / 1000;
        let (year, month, day) = civil_from_days(days);
        Some(DateTime {
            year,
            month,
            day,
            hour: (rest / 3600) as u32,
            minute: (rest / 60 % 60) as u32,
            second: (rest % 60) as u32,
            weekday: (days + 4).rem_euclid(7) as u32,
        })
    }

    /// `2026-11-05T14:07:09` and the date-only `2026-11-05` form.
    pub fn parse_iso(text: &str) -> Option<DateTime> {
        let text = text.trim();
        let (date, time) = match text.split_once(['T', ' ']) {
            Some((date, time)) => (date, Some(time)),
            None => (text, None),
        };
        let mut parts = date.split('-');
        let year: i64 = parts.next()?.parse().ok()?;
        let month: u32 = parts.next()?.parse().ok()?;
        let day: u32 = parts.next()?.parse().ok()?;
        if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return None;
        }
        let (mut hour, mut minute, mut second) = (0, 0, 0);
        if let Some(time) = time {
            let time = time.trim_end_matches('Z');
            let mut clock = time.split(':');
            hour = clock.next()?.parse().ok()?;
            minute = clock.next().unwrap_or("0").parse().ok()?;
            second = clock
                .next()
                .unwrap_or("0")
                .split('.')
                .next()?
                .parse()
                .ok()?;
        }
        Some(DateTime::new(year, month, day, hour, minute, second))
    }

    /// The ISO form the formula bar shows for a date, as Excel shows its own.
    pub fn input(&self) -> String {
        let day = format!("{:04}-{:02}-{:02}", self.year, self.month, self.day);
        if self.hour == 0 && self.minute == 0 && self.second == 0 {
            day
        } else {
            format!(
                "{day} {:02}:{:02}:{:02}",
                self.hour, self.minute, self.second
            )
        }
    }
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Hinnant's algorithm).
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let shifted = (i64::from(month) + 9) % 12;
    let day_of_year = (153 * shifted + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_index + 2) / 5 + 1) as u32;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    } as u32;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

fn narrow(name: &str) -> String {
    name.chars()
        .next()
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_default()
}

fn starts_alpha(token: &str) -> bool {
    token
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic())
}

/// A date as its format prints it. A format with no date tokens prints the
/// locale's short date, with the time when there is one.
pub fn format_date(date: &DateTime, format: Option<&str>, locale: &Locale) -> String {
    let section = format
        .map(|f| split_sections(f).swap_remove(0))
        .unwrap_or_default();
    if section.is_empty() || !is_date_format(Some(&section)) {
        let year = date.year.rem_euclid(100);
        let day_month = if locale.day_first {
            format!("{}/{}/{year:02}", date.day, date.month)
        } else {
            format!("{}/{}/{year:02}", date.month, date.day)
        };
        if date.hour == 0 && date.minute == 0 && date.second == 0 {
            return day_month;
        }
        return if locale.hour12 {
            let suffix = if date.hour < 12 { "AM" } else { "PM" };
            let clock = if date.hour.is_multiple_of(12) {
                12
            } else {
                date.hour % 12
            };
            format!("{day_month}, {clock}:{:02} {suffix}", date.minute)
        } else {
            format!("{day_month}, {}:{:02}", date.hour, date.minute)
        };
    }
    let twelve = TWELVE.is_match(&section);
    let hours = date.hour;
    let month_index = (date.month - 1) as usize;
    let pad = |value: u32| format!("{value:02}");
    let stripped = BRACKETS.replace_all(&section, "");
    let tokens: Vec<String> = DATE_TOKENS
        .find_iter(&stripped)
        .map(|found| found.as_str().to_string())
        .collect();
    let mut out = String::new();
    for (index, token) in tokens.iter().enumerate() {
        let lower = token.to_lowercase();
        if token.starts_with('"') {
            out.push_str(token.get(1..token.len().saturating_sub(1)).unwrap_or(""));
            continue;
        }
        if let Some(escaped) = token.strip_prefix('\\') {
            out.push_str(escaped);
            continue;
        }
        // `m` after an hour or before seconds is minutes.
        let minutes = (lower == "m" || lower == "mm")
            && (tokens[..index]
                .iter()
                .rev()
                .find(|t| starts_alpha(t))
                .is_some_and(|t| t.to_lowercase().starts_with('h'))
                || tokens[index + 1..]
                    .iter()
                    .find(|t| starts_alpha(t))
                    .is_some_and(|t| t.to_lowercase().starts_with('s')));
        if minutes {
            out.push_str(&if lower == "mm" {
                pad(date.minute)
            } else {
                date.minute.to_string()
            });
            continue;
        }
        let clock = if twelve {
            if hours.is_multiple_of(12) {
                12
            } else {
                hours % 12
            }
        } else {
            hours
        };
        match lower.as_str() {
            "yyyy" => out.push_str(&date.year.to_string()),
            "yy" => out.push_str(&pad(date.year.rem_euclid(100) as u32)),
            "mmmmm" => out.push_str(&narrow(locale.months[month_index])),
            "mmmm" => out.push_str(locale.months[month_index]),
            "mmm" => out.push_str(locale.months_short[month_index]),
            "mm" => out.push_str(&pad(date.month)),
            "m" => out.push_str(&date.month.to_string()),
            "dddd" => out.push_str(locale.weekdays[date.weekday as usize]),
            "ddd" => out.push_str(locale.weekdays_short[date.weekday as usize]),
            "dd" => out.push_str(&pad(date.day)),
            "d" => out.push_str(&date.day.to_string()),
            "hh" => out.push_str(&pad(clock)),
            "h" => out.push_str(&clock.to_string()),
            "ss" => out.push_str(&pad(date.second)),
            "s" => out.push_str(&date.second.to_string()),
            "am/pm" => out.push_str(if hours < 12 { "AM" } else { "PM" }),
            "a/p" => out.push_str(if hours < 12 { "A" } else { "P" }),
            _ => out.push_str(token),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(value: f64, format: Option<&str>) -> String {
        format_number(value, format, &Locale::en())
    }

    #[test]
    fn prints_general_without_padding() {
        assert_eq!(f(3.0, None), "3");
        assert_eq!(f(0.1 + 0.2, None), "0.3");
        assert_eq!(f(1234567.5, Some("General")), "1234567.5");
        assert_eq!(f(1.5e20, None), "1.5E+20");
        assert_eq!(f(0.0000000001, None), "1E-10");
        assert_eq!(f(-12.0, None), "-12");
    }

    #[test]
    fn keeps_decimals_thousands_percent_and_units() {
        assert_eq!(f(1066.0, Some("#,##0")), "1,066");
        assert_eq!(f(340000.456, Some("#,##0.00")), "340,000.46");
        assert_eq!(f(0.256, Some("0.0%")), "25.6%");
        assert_eq!(f(74.0, Some("0 \"m\"")), "74 m");
        assert_eq!(f(12.5, Some("[$S/-280A] #,##0.00")), "S/ 12.50");
        assert_eq!(f(1.5, Some("0.##")), "1.5");
        assert_eq!(f(1.005, Some("0.00")), "1.01");
        assert_eq!(f(2500000.0, Some("#,##0,,\"M\"")), "3M");
        assert_eq!(f(9.999, Some("0.00")), "10.00");
    }

    #[test]
    fn uses_the_negative_and_zero_sections() {
        assert_eq!(f(-1500.0, Some("#,##0;(#,##0)")), "(1,500)");
        assert_eq!(f(-2.0, Some("0;[Red]-0")), "-2");
        assert_eq!(f(0.0, Some("0;-0;\"-\"")), "-");
        assert_eq!(f(-1500.0, Some("#,##0")), "-1,500");
    }

    #[test]
    fn prints_in_the_spanish_locale() {
        let es = Locale::es();
        assert_eq!(format_number(1234.5, Some("#,##0.00"), &es), "1234,50");
        assert_eq!(
            format_number(1234567.891, Some("#,##0.00"), &es),
            "1.234.567,89"
        );
        assert_eq!(format_number(0.5, None, &es), "0,5");
    }

    #[test]
    fn prints_dates_in_utc_as_the_format_says() {
        let date = DateTime::new(2026, 11, 5, 14, 7, 9);
        let en = Locale::en();
        assert_eq!(format_date(&date, Some("dd/mm/yyyy"), &en), "05/11/2026");
        assert_eq!(
            format_date(&date, Some("yyyy-mm-dd hh:mm:ss"), &en),
            "2026-11-05 14:07:09"
        );
        assert_eq!(format_date(&date, Some("h:mm AM/PM"), &en), "2:07 PM");
        assert_eq!(format_date(&date, Some("d-mmm-yy"), &en), "5-Nov-26");
        assert_eq!(
            format_date(&date, Some("dddd d mmmm"), &Locale::es()),
            "jueves 5 noviembre"
        );
        assert_eq!(format_date(&date, None, &en), "11/5/26, 2:07 PM");
        assert_eq!(
            format_date(&date, Some("General"), &Locale::es()),
            "5/11/26, 14:07"
        );
    }

    #[test]
    fn converts_serial_numbers_and_back() {
        let date = DateTime::from_serial(46331.0, false).expect("a date");
        assert_eq!(date, DateTime::new(2026, 11, 5, 0, 0, 0));
        assert_eq!(date.input(), "2026-11-05");
        assert_eq!(date.weekday, 4);
        let noon = DateTime::from_serial(46331.5, false).expect("a date");
        assert_eq!(noon.input(), "2026-11-05 12:00:00");
        let mac = DateTime::from_serial(46331.0 - 1462.0, true).expect("a date");
        assert_eq!(mac.input(), "2026-11-05");
        assert_eq!(
            DateTime::parse_iso("2026-11-05T14:07:09Z")
                .map(|d| d.input())
                .as_deref(),
            Some("2026-11-05 14:07:09")
        );
    }

    #[test]
    fn tells_date_formats_from_number_formats() {
        assert!(is_date_format(Some("dd/mm/yyyy")));
        assert!(is_date_format(Some("h:mm AM/PM")));
        assert!(!is_date_format(Some("#,##0.00")));
        assert!(!is_date_format(Some("0 \"m\"")));
        assert!(!is_date_format(Some("General")));
        assert!(!is_date_format(None));
    }
}
