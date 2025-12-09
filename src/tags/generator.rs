use crate::parser_state::ParserState;

// Handles <generator> text within <channel>
pub fn on_text(data: &str, state: &mut ParserState) {
    if state.in_channel && !state.in_item && !state.in_channel_image {
        state.channel_generator.push_str(data);
    }
}
