use crate::parser_state::ParserState;

pub fn on_text(data: &str, state: &mut ParserState) {
    if state.in_item {
        state.title.push_str(data);
    } else if state.in_channel && !state.in_channel_image {
        state.channel_title.push_str(data);
    }
}
