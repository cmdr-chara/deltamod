//! Fail-closed policy for text that crosses into serializable provider contracts.

const MAX_VERSION_BYTES: usize = 64;
const MAX_CREDENTIAL_SCAN_BYTES: usize = 4_096;
const ASSIGNMENT_CREDENTIAL_KEYS: &[&str] = &[
    "token",
    "secret",
    "accesstoken",
    "authtoken",
    "refreshtoken",
    "sessiontoken",
    "apikey",
    "clientsecret",
    "privatekey",
    "privategamekey",
    "password",
    "passwd",
    "authorization",
    "credential",
    "signature",
    "bearer",
];
const IDENTIFIER_CREDENTIAL_KEYS: &[&str] = &[
    "accesstoken",
    "authtoken",
    "refreshtoken",
    "sessiontoken",
    "apikey",
    "clientsecret",
    "privatekey",
    "privategamekey",
    "password",
    "passwd",
    "authorization",
    "credential",
];
const SEPARATOR_OBFUSCATED_IDENTIFIER_CREDENTIAL_KEYS: &[&str] = &["token", "secret", "signature"];
const BEARER_KEY: &str = "bearer";

fn ascii_key_characters(value: &str) -> Vec<(usize, u8)> {
    value
        .char_indices()
        .filter_map(|(index, character)| {
            character
                .is_ascii_alphanumeric()
                .then_some((index, character.to_ascii_lowercase() as u8))
        })
        .collect()
}

fn key_characters_match(characters: &[(usize, u8)], key: &str) -> bool {
    characters.len() == key.len()
        && characters
            .iter()
            .map(|(_, character)| *character)
            .eq(key.bytes())
}

fn key_starts_at_boundary(value: &str, index: usize) -> bool {
    value[..index]
        .chars()
        .next_back()
        .is_none_or(|character| !character.is_ascii_alphanumeric())
}

fn key_ends_at_boundary(value: &str, index: usize) -> bool {
    value[index + 1..]
        .chars()
        .next()
        .is_none_or(|character| !character.is_ascii_alphanumeric())
}

fn bounded_key_window_matches(value: &str, characters: &[(usize, u8)], key: &str) -> bool {
    let (Some((start, _)), Some((end, _))) = (characters.first(), characters.last()) else {
        return false;
    };
    key_characters_match(characters, key)
        && key_starts_at_boundary(value, *start)
        && key_ends_at_boundary(value, *end)
}

fn contains_identifier_credential_key(value: &str, characters: &[(usize, u8)]) -> bool {
    IDENTIFIER_CREDENTIAL_KEYS.iter().any(|key| {
        characters
            .windows(key.len())
            .any(|window| bounded_key_window_matches(value, window, key))
    })
}

fn contains_separator_obfuscated_identifier_credential_key(
    value: &str,
    characters: &[(usize, u8)],
) -> bool {
    SEPARATOR_OBFUSCATED_IDENTIFIER_CREDENTIAL_KEYS
        .iter()
        .any(|key| {
            characters.windows(key.len()).any(|window| {
                bounded_key_window_matches(value, window, key)
                    && window.windows(2).any(|pair| pair[1].0 > pair[0].0 + 1)
            })
        })
}

pub(crate) fn credential_shaped_scope(value: &str) -> bool {
    if credential_shaped_identifier(value) {
        return true;
    }
    let normalized = value.to_ascii_lowercase();
    let terms = normalized
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|term| !term.is_empty())
        .collect::<Vec<_>>();
    terms.iter().any(|term| {
        matches!(
            *term,
            "token" | "secret" | "password" | "passwd" | "credential" | "authorization" | "bearer"
        )
    }) || terms.windows(2).any(|pair| {
        pair[1] == "key"
            && matches!(
                pair[0],
                "api" | "auth" | "client" | "game" | "private" | "session" | "access"
            )
    })
}

fn has_credential_assignment(value: &str, characters: &[(usize, u8)]) -> bool {
    value.char_indices().any(|(index, character)| {
        if !matches!(character, '=' | ':') {
            return false;
        }
        let key_end = characters.partition_point(|(position, _)| *position < index);
        credential_assignment_key(value, &characters[..key_end])
            && credential_value_present(&value[index + character.len_utf8()..])
    })
}

fn credential_assignment_key(value: &str, characters: &[(usize, u8)]) -> bool {
    ASSIGNMENT_CREDENTIAL_KEYS.iter().any(|key| {
        if characters.len() < key.len() {
            return false;
        }
        let suffix = &characters[characters.len() - key.len()..];
        let Some((start, _)) = suffix.first() else {
            return false;
        };
        key_characters_match(suffix, key) && key_starts_at_boundary(value, *start)
    })
}

fn credential_value_present(value: &str) -> bool {
    let value = value.trim_start();
    let Some(first) = value.chars().next() else {
        return false;
    };
    let remainder = &value[first.len_utf8()..];

    if matches!(first, '"' | '\'' | '`') {
        return remainder.find(first).is_none_or(|end| {
            remainder[..end]
                .chars()
                .any(|character| !character.is_whitespace())
                || wrapped_value_has_adjacent_content(&remainder[end + first.len_utf8()..])
        });
    }

    if let Some(closer) = match first {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        _ => None,
    } {
        let mut depth = 1_usize;
        for (index, character) in remainder.char_indices() {
            if character == first {
                depth = depth.saturating_add(1);
            } else if character == closer {
                depth -= 1;
                if depth == 0 {
                    return remainder[..index]
                        .chars()
                        .any(|character| !character.is_whitespace())
                        || wrapped_value_has_adjacent_content(
                            &remainder[index + closer.len_utf8()..],
                        );
                }
            }
        }
        return true;
    }

    if clear_credential_value_boundary(first) {
        return false;
    }

    value
        .chars()
        .take_while(|character| !clear_credential_value_boundary(*character))
        .any(|character| !character.is_whitespace())
}

fn wrapped_value_has_adjacent_content(value: &str) -> bool {
    value.chars().next().is_some_and(|character| {
        !character.is_whitespace() && !clear_credential_value_boundary(character)
    })
}

fn clear_credential_value_boundary(character: char) -> bool {
    matches!(character, ')' | ']' | '}' | '\n' | '\r' | ',' | ';' | '|')
}

fn has_bearer_value(value: &str, characters: &[(usize, u8)]) -> bool {
    characters.windows(BEARER_KEY.len()).any(|window| {
        if !bounded_key_window_matches(value, window, BEARER_KEY) {
            return false;
        }

        let start = window[0].0;
        let end = window[window.len() - 1].0 + 1;
        let prefix = &value[..start];
        let mut suffix = &value[end..];
        let expected_closer = match prefix.chars().next_back() {
            Some('"') => Some('"'),
            Some('\'') => Some('\''),
            Some('`') => Some('`'),
            Some('(') => Some(')'),
            Some('[') => Some(']'),
            Some('{') => Some('}'),
            _ => None,
        };
        if let Some(closer) = expected_closer {
            if suffix.starts_with(closer) {
                suffix = &suffix[closer.len_utf8()..];
            }
        }
        suffix = suffix.trim_start();
        if suffix.starts_with(':') || suffix.starts_with('=') {
            suffix = suffix[1..].trim_start();
        }
        credential_value_present(suffix)
    })
}

pub(crate) fn credential_shaped_text(value: &str) -> bool {
    if value.len() > MAX_CREDENTIAL_SCAN_BYTES {
        return true;
    }
    let characters = ascii_key_characters(value);
    has_credential_assignment(value, &characters) || has_bearer_value(value, &characters)
}

pub(crate) fn credential_shaped_identifier(value: &str) -> bool {
    if value.len() > MAX_CREDENTIAL_SCAN_BYTES {
        return true;
    }
    let characters = ascii_key_characters(value);
    contains_identifier_credential_key(value, &characters)
        || contains_separator_obfuscated_identifier_credential_key(value, &characters)
        || has_credential_assignment(value, &characters)
        || has_bearer_value(value, &characters)
}

fn path_or_url_shaped(value: &str) -> bool {
    let trimmed = value.trim();
    let lower = trimmed.to_ascii_lowercase();
    lower.contains("://")
        || lower.starts_with("http:")
        || lower.starts_with("https:")
        || trimmed.contains(['/', '\\'])
}

pub(crate) fn stable_text_allowed(value: &str) -> bool {
    !value.is_empty()
        && !value.chars().any(char::is_control)
        && !path_or_url_shaped(value)
        && !credential_shaped_text(value)
}

pub(crate) fn normalize_version_label(value: &str) -> Option<String> {
    if value.is_empty() || value.len() > MAX_VERSION_BYTES || value.chars().any(char::is_control) {
        return None;
    }
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty()
        || credential_shaped_identifier(&normalized)
        || path_or_url_shaped(&normalized)
        || !normalized.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'.' | b'-' | b'_' | b'+' | b' ' | b'(' | b')' | b'[' | b']'
                )
        })
        || !normalized
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        || !normalized
            .bytes()
            .last()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
    {
        None
    } else {
        Some(normalized)
    }
}

pub(crate) fn safe_stable_basename(value: &str, maximum: usize) -> Option<String> {
    if value.is_empty() || value.len() > maximum || value.chars().any(char::is_control) {
        return None;
    }
    let value = value.trim();
    (!value.is_empty()
        && value.len() <= maximum
        && !matches!(value, "." | "..")
        && !credential_shaped_identifier(value)
        && !path_or_url_shaped(value)
        && !value.contains(['/', '\\'])
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'.' | b'-' | b'_' | b'+' | b' ' | b'(' | b')' | b'[' | b']'
                )
        })
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric()))
    .then(|| value.to_owned())
}

pub(crate) fn safe_contract_identifier(value: &str, maximum: usize) -> bool {
    !credential_shaped_identifier(value)
        && !value.is_empty()
        && value.len() <= maximum
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standalone_assignments_and_bearer_values_are_credential_shaped() {
        for value in [
            "token=ROUND7_VALUE",
            "  ToKeN = ROUND7_VALUE  ",
            r#"{ "SeCrEt" : "ROUND7_VALUE" }"#,
            "prefix; API_KEY = 'ROUND7_VALUE'",
            "prefix | apikey:ROUND7_VALUE",
            "(client secret) = ROUND7_VALUE",
            "t-o-k-e-n=密钥",
            r#"s.e.c.r.e.t="!""#,
            r#""a-p-i-k-e-y" : "[密钥]""#,
            r#"X-S-i-g-n-a-t-u-r-e: "!密钥!""#,
            "https://example.invalid/callback?t-o-k-e-n=密钥",
            "Bearer ROUND7_VALUE",
            "b-e-a-r-e-r 密钥",
            r#""bEaReR" "ROUND7_VALUE""#,
        ] {
            assert!(!stable_text_allowed(value), "accepted {value:?}");
        }

        for value in ["Bearer ROUND7_VERSION", "bearer-ROUND7-VALUE"] {
            assert!(
                normalize_version_label(value).is_none(),
                "accepted {value:?}"
            );
            assert!(
                safe_stable_basename(value, 255).is_none(),
                "accepted {value:?}"
            );
            assert!(!safe_contract_identifier(value, 128), "accepted {value:?}");
        }
    }

    #[test]
    fn every_credential_key_allows_arbitrary_punctuation_between_letters() {
        for key in ASSIGNMENT_CREDENTIAL_KEYS {
            let split = key
                .chars()
                .map(|character| character.to_string())
                .collect::<Vec<_>>()
                .join("•-_.:");
            let value = format!(r#""{split}" = "!密钥!""#);
            assert!(credential_shaped_text(&value), "accepted {value:?}");
        }
    }

    #[test]
    fn separator_obfuscated_short_credential_words_are_identifier_shaped() {
        for value in [
            "t-o-k-e-n-round9credential",
            "s-e-c-r-e-t-round9credential",
            "s-i-g-n-a-t-u-r-e-round9credential",
            "TO-K_E.N-round9credential",
            "SE-C_R.E-T-round9credential",
            "SIGNA-T_U.R-E-round9credential",
        ] {
            assert!(
                credential_shaped_identifier(value),
                "accepted identifier {value:?}"
            );
            assert!(
                normalize_version_label(value).is_none(),
                "accepted {value:?}"
            );
            assert!(
                safe_stable_basename(value, 255).is_none(),
                "accepted {value:?}"
            );
            assert!(!safe_contract_identifier(value, 128), "accepted {value:?}");
        }
    }

    #[test]
    fn benign_credential_words_require_assignment_boundaries() {
        for value in [
            "token economy",
            "secret level",
            "passwordless mode",
            "signature move",
            "tokenization",
            "secretariat",
            "tokenizer=enabled",
            "mytoken=enabled",
            "token economy=healthy",
            "secret_level=hidden",
            "secret level: hidden",
        ] {
            assert!(stable_text_allowed(value), "rejected {value:?}");
        }

        for value in [
            "token-economy",
            "secret-level",
            "passwordless-mode",
            "signature-move",
            "tokenization",
            "secretariat",
            "t-o-k-e-nization",
            "s-e-c-r-e-tariat",
        ] {
            assert!(
                !credential_shaped_identifier(value),
                "rejected identifier {value:?}"
            );
            assert_eq!(normalize_version_label(value).as_deref(), Some(value));
            assert_eq!(safe_stable_basename(value, 255).as_deref(), Some(value));
            assert!(safe_contract_identifier(value, 128));
        }
    }

    #[test]
    fn symbol_and_unicode_values_are_credential_shaped() {
        for value in [
            "token=!",
            "secret=!!!",
            "token=密钥",
            r#""token" = "密钥""#,
            "prefix; secret = [!!!]",
            r#"token=",""#,
            "secret=[;]",
            r#"token=""#,
            r#"token=""密钥"#,
            "secret=[",
            "secret=[]!",
            "Bearer !",
            "Bearer 密钥",
            r#"Bearer "|""#,
            r#""Bearer" ",""#,
            r#""bEaReR" [密钥]"#,
        ] {
            assert!(!stable_text_allowed(value), "accepted {value:?}");
        }
    }

    #[test]
    fn empty_values_and_clear_prose_boundaries_remain_benign() {
        for value in [
            "token= ; ordinary prose",
            "Bearer, however, is an ordinary noun here",
            "token=[]",
            r#"token="" ordinary prose"#,
            "Bearer",
            "Bearer [] ordinary prose",
            "Ordinary punctuation: wow!",
        ] {
            assert!(stable_text_allowed(value), "rejected {value:?}");
        }
    }
}
