use crate::parser_state::ParserState;

pub fn on_text(data: &str, state: &mut ParserState) {
    if state.in_item {
        if state.link.is_empty() {
            state.link.push_str(data);
        }
    } else if state.in_channel && !state.in_channel_image {
        if state.channel_link.is_empty() {
            state.channel_link.push_str(data);
        }
    }
}
