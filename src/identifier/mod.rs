pub fn identifier_tokens(value: &str) -> Vec<String> {
    let characters: Vec<char> = value.chars().collect();
    let mut tokens = Vec::new();
    let mut current = String::new();

    for (index, character) in characters.iter().copied().enumerate() {
        if !character.is_ascii_alphanumeric() {
            push_token(&mut tokens, &mut current);
            continue;
        }

        let previous = index.checked_sub(1).map(|position| characters[position]);
        let next = characters.get(index + 1).copied();
        let after_next = characters.get(index + 2).copied();
        let plural_acronym_suffix = next == Some('s')
            && after_next
                .is_none_or(|value| !value.is_ascii_alphanumeric() || value.is_ascii_uppercase());
        let boundary = !current.is_empty()
            && (matches!(previous, Some(value) if value.is_ascii_lowercase())
                && character.is_ascii_uppercase()
                || matches!(previous, Some(value) if value.is_ascii_alphabetic())
                    && character.is_ascii_digit()
                || matches!(previous, Some(value) if value.is_ascii_digit())
                    && character.is_ascii_alphabetic()
                || matches!(previous, Some(value) if value.is_ascii_uppercase())
                    && character.is_ascii_uppercase()
                    && matches!(next, Some(value) if value.is_ascii_lowercase())
                    && !plural_acronym_suffix);
        if boundary {
            push_token(&mut tokens, &mut current);
        }
        current.push(character);
    }
    push_token(&mut tokens, &mut current);
    tokens
}

pub fn canonical_identifier(value: &str) -> String {
    identifier_tokens(value).join("")
}

fn push_token(tokens: &mut Vec<String>, current: &mut String) {
    if !current.is_empty() {
        tokens.push(current.to_ascii_lowercase());
        current.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::{canonical_identifier, identifier_tokens};

    #[test]
    fn splits_graphql_identifiers_deterministically() {
        assert_eq!(identifier_tokens("assetId"), ["asset", "id"]);
        assert_eq!(identifier_tokens("URLValue"), ["url", "value"]);
        assert_eq!(
            identifier_tokens("notificationIDs"),
            ["notification", "ids"]
        );
        assert_eq!(
            identifier_tokens("notificationIDsFilter"),
            ["notification", "ids", "filter"]
        );
        assert_eq!(identifier_tokens("per_page"), ["per", "page"]);
        assert_eq!(identifier_tokens("card2Id"), ["card", "2", "id"]);
        assert_eq!(canonical_identifier("Notification-Ids"), "notificationids");
    }
}
