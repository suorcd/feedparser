use crate::parser_state::ParserState;

pub fn on_start(state: &mut ParserState) {
    // Track when entering an <image> inside the channel (but not inside items)
    if state.in_channel && !state.in_item {
        state.in_channel_image = true;
    }
}

pub fn on_end(state: &mut ParserState) {
    if state.in_channel_image {
        state.in_channel_image = false;
    }
}
