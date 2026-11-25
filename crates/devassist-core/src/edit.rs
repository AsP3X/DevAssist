//! Editing one line of text, apart from the keys that ask for it.

pub fn delete_word(text: &mut String) {
    while text
        .chars()
        .next_back()
        .is_some_and(|ch| ch.is_whitespace())
    {
        text.pop();
    }
    while text
        .chars()
        .next_back()
        .is_some_and(|ch| !ch.is_whitespace())
    {
        text.pop();
    }
    while text
        .chars()
        .next_back()
        .is_some_and(|ch| ch.is_whitespace())
    {
        text.pop();
    }
}
