//! Language detection for text content

use whatlang::{detect, Lang};

/// Detect the language of text content
///
/// Returns the ISO 639-1 language code if detection is confident enough.
pub fn detect_language(text: &str) -> Option<String> {
    detect_language_with_threshold(text, 0.8)
}

/// Detect language with custom confidence threshold
pub fn detect_language_with_threshold(text: &str, threshold: f64) -> Option<String> {
    // Need enough text for reliable detection
    if text.len() < 50 {
        return None;
    }

    let info = detect(text)?;

    // Check confidence
    if info.confidence() < threshold {
        return None;
    }

    // Convert to ISO 639-1 code
    Some(lang_to_iso_639_1(info.lang()))
}

/// Get detailed language detection info
pub fn detect_language_info(text: &str) -> Option<LanguageInfo> {
    if text.len() < 50 {
        return None;
    }

    let info = detect(text)?;

    Some(LanguageInfo {
        code: lang_to_iso_639_1(info.lang()),
        name: lang_to_name(info.lang()),
        confidence: info.confidence(),
        is_reliable: info.is_reliable(),
    })
}

/// Language detection information
#[derive(Debug, Clone)]
pub struct LanguageInfo {
    /// ISO 639-1 language code
    pub code: String,
    /// Language name
    pub name: String,
    /// Detection confidence (0.0 - 1.0)
    pub confidence: f64,
    /// Whether the detection is considered reliable
    pub is_reliable: bool,
}

/// Convert whatlang Lang to ISO 639-1 code
fn lang_to_iso_639_1(lang: Lang) -> String {
    match lang {
        Lang::Afr => "af",
        Lang::Aka => "ak",
        Lang::Amh => "am",
        Lang::Ara => "ar",
        Lang::Aze => "az",
        Lang::Bel => "be",
        Lang::Ben => "bn",
        Lang::Bul => "bg",
        Lang::Cat => "ca",
        Lang::Ces => "cs",
        Lang::Cmn => "zh",
        Lang::Dan => "da",
        Lang::Deu => "de",
        Lang::Ell => "el",
        Lang::Eng => "en",
        Lang::Epo => "eo",
        Lang::Est => "et",
        Lang::Fin => "fi",
        Lang::Fra => "fr",
        Lang::Guj => "gu",
        Lang::Heb => "he",
        Lang::Hin => "hi",
        Lang::Hrv => "hr",
        Lang::Hun => "hu",
        Lang::Hye => "hy",
        Lang::Ind => "id",
        Lang::Ita => "it",
        Lang::Jpn => "ja",
        Lang::Jav => "jv",
        Lang::Kan => "kn",
        Lang::Kat => "ka",
        Lang::Khm => "km",
        Lang::Kor => "ko",
        Lang::Lat => "la",
        Lang::Lav => "lv",
        Lang::Lit => "lt",
        Lang::Mal => "ml",
        Lang::Mar => "mr",
        Lang::Mkd => "mk",
        Lang::Mya => "my",
        Lang::Nep => "ne",
        Lang::Nld => "nl",
        Lang::Nob => "nb",
        Lang::Ori => "or",
        Lang::Pan => "pa",
        Lang::Pes => "fa",
        Lang::Pol => "pl",
        Lang::Por => "pt",
        Lang::Ron => "ro",
        Lang::Rus => "ru",
        Lang::Sin => "si",
        Lang::Slk => "sk",
        Lang::Slv => "sl",
        Lang::Sna => "sn",
        Lang::Spa => "es",
        Lang::Srp => "sr",
        Lang::Swe => "sv",
        Lang::Tam => "ta",
        Lang::Tel => "te",
        Lang::Tgl => "tl",
        Lang::Tha => "th",
        Lang::Tuk => "tk",
        Lang::Tur => "tr",
        Lang::Ukr => "uk",
        Lang::Urd => "ur",
        Lang::Uzb => "uz",
        Lang::Vie => "vi",
        Lang::Yid => "yi",
        Lang::Zul => "zu",
    }
    .to_string()
}

/// Convert whatlang Lang to human-readable name
fn lang_to_name(lang: Lang) -> String {
    match lang {
        Lang::Eng => "English",
        Lang::Fra => "French",
        Lang::Deu => "German",
        Lang::Spa => "Spanish",
        Lang::Ita => "Italian",
        Lang::Por => "Portuguese",
        Lang::Rus => "Russian",
        Lang::Jpn => "Japanese",
        Lang::Cmn => "Chinese",
        Lang::Kor => "Korean",
        Lang::Ara => "Arabic",
        Lang::Hin => "Hindi",
        Lang::Ben => "Bengali",
        Lang::Tur => "Turkish",
        Lang::Pol => "Polish",
        Lang::Nld => "Dutch",
        Lang::Swe => "Swedish",
        Lang::Dan => "Danish",
        Lang::Fin => "Finnish",
        Lang::Nob => "Norwegian Bokmål",
        Lang::Ukr => "Ukrainian",
        Lang::Ces => "Czech",
        Lang::Ron => "Romanian",
        Lang::Hun => "Hungarian",
        Lang::Ell => "Greek",
        Lang::Heb => "Hebrew",
        Lang::Tha => "Thai",
        Lang::Vie => "Vietnamese",
        Lang::Ind => "Indonesian",
        Lang::Mal => "Malayalam",
        _ => "Unknown",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_english() {
        let text = "This is a sample English text that should be long enough for reliable language detection by the algorithm.";
        let lang = detect_language(text);
        assert_eq!(lang, Some("en".to_string()));
    }

    #[test]
    fn test_detect_french() {
        let text = "Ceci est un exemple de texte en français qui devrait être assez long pour une détection fiable de la langue par l'algorithme.";
        let lang = detect_language(text);
        assert_eq!(lang, Some("fr".to_string()));
    }

    #[test]
    fn test_detect_german() {
        let text = "Dies ist ein Beispieltext auf Deutsch, der lang genug sein sollte, um von dem Algorithmus zuverlässig erkannt zu werden.";
        let lang = detect_language(text);
        assert_eq!(lang, Some("de".to_string()));
    }

    #[test]
    fn test_detect_spanish() {
        let text = "Este es un texto de ejemplo en español que debería ser lo suficientemente largo para una detección confiable del idioma por el algoritmo.";
        let lang = detect_language(text);
        assert_eq!(lang, Some("es".to_string()));
    }

    #[test]
    fn test_short_text_returns_none() {
        let text = "Short text";
        let lang = detect_language(text);
        assert!(lang.is_none());
    }

    #[test]
    fn test_language_info() {
        let text = "This is a sample English text that should be long enough for reliable language detection by the algorithm.";
        let info = detect_language_info(text);

        assert!(info.is_some());
        let info = info.unwrap();
        assert_eq!(info.code, "en");
        assert_eq!(info.name, "English");
        assert!(info.confidence > 0.5);
    }
}
