use xml::attribute::OwnedAttribute;
use xml::name::OwnedName;

use crate::parser_state::ParserState;

pub fn on_start(name: &OwnedName, attributes: &[OwnedAttribute], state: &mut ParserState) {
    if !state.in_item {
        return;
    }

    let is_itunes_image = name.local_name == "image"
        && (matches!(name.prefix.as_deref(), Some("itunes"))
            || matches!(
                name.namespace.as_deref(),
                Some("http://www.itunes.com/dtds/podcast-1.0.dtd")
            ));

    if is_itunes_image {
        if let Some(attr) = attributes.iter().find(|a| {
            let key = a.name.local_name.as_str();
            key == "href" || key == "url"
        }) {
            state.itunes_image = attr.value.clone();
        }
    }
}
