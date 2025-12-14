use crate::{OUTPUT_SUBDIR, process_feed_sync};
use std::path::PathBuf;

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Cursor;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_dir() -> PathBuf {
        let base = std::env::temp_dir();
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let dir = base.join(format!("feedparser_test_{}", ts));
        let _ = fs::create_dir_all(&dir);
        dir
    }

    fn ensure_output_dir() -> PathBuf {
        if let Some(existing) = OUTPUT_SUBDIR.get() {
            return existing.clone();
        }
        let dir = unique_temp_dir();
        let _ = OUTPUT_SUBDIR.set(dir.clone());
        dir
    }

    #[test]
    fn writes_channel_title_to_newsfeeds_output() {
        // Arrange: ensure outputs directory is set once for all tests in this process
        let out_dir = ensure_output_dir();

        // Synthetic input: 4 header lines followed by minimal RSS with channel title
        let last_modified = "0"; // placeholder
        let etag = "[[NO_ETAG]]";
        let url = "https://example.com/feed.xml";
        let downloaded = "0";
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <title>My Test Channel</title>
  </channel>
</rss>"#;

        let input = format!(
            "{last}\n{etag}\n{url}\n{dl}\n{xml}\n",
            last = last_modified,
            etag = etag,
            url = url,
            dl = downloaded,
            xml = xml
        );

        let feed_id = Some(424242_i64);

        // Act: process the synthetic feed synchronously
        process_feed_sync(Cursor::new(input.into_bytes()), "<test>", feed_id);

        // Assert: a newsfeeds JSON file exists with the expected title
        let entries = fs::read_dir(&out_dir)
            .expect("output directory should be readable");
        let mut found_path: Option<PathBuf> = None;
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                if name.contains("_newsfeeds_") && name.ends_with("424242.json") {
                    found_path = Some(path);
                    break;
                }
            }
        }

        let file_path = found_path.expect("should have written a newsfeeds output file");
        let contents = fs::read_to_string(&file_path)
            .expect("should be able to read newsfeeds file");
        let v: serde_json::Value = serde_json::from_str(&contents)
            .expect("valid JSON in newsfeeds file");

        // Basic shape assertions
        assert_eq!(v["table"], "newsfeeds");
        assert_eq!(v["columns"][1], "title");
        assert_eq!(v["feed_id"], serde_json::json!(424242));

        // Channel title should be the second value (index 1), trimmed
        assert_eq!(v["values"][1], serde_json::json!("My Test Channel"));
    }

    #[test]
    fn writes_channel_link_and_description_cdata() {
        // Arrange
        let out_dir = ensure_output_dir();

        let last_modified = "0";
        let etag = "[[NO_ETAG]]";
        let url = "https://example.com/feed.xml";
        let downloaded = "0";
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <title>Channel With Links</title>
    <link>https://example.com/</link>
    <description><![CDATA[ This is a <b>CDATA</b> description. ]]></description>
  </channel>
</rss>"#;

        let input = format!(
            "{last}\n{etag}\n{url}\n{dl}\n{xml}\n",
            last = last_modified,
            etag = etag,
            url = url,
            dl = downloaded,
            xml = xml
        );

        let feed_id = Some(777001_i64);

        // Act
        process_feed_sync(Cursor::new(input.into_bytes()), "<test>", feed_id);

        // Assert: find the newsfeeds file for this feed_id
        let entries = fs::read_dir(&out_dir).expect("output directory should be readable");
        let mut found_path: Option<PathBuf> = None;
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                if name.contains("_newsfeeds_") && name.ends_with("777001.json") {
                    found_path = Some(path);
                    break;
                }
            }
        }

        let file_path = found_path.expect("should have written a newsfeeds output file");
        let contents = fs::read_to_string(&file_path).expect("read newsfeeds file");
        let v: serde_json::Value = serde_json::from_str(&contents).expect("valid JSON");

        assert_eq!(v["table"], "newsfeeds");
        // title
        assert_eq!(v["values"][1], serde_json::json!("Channel With Links"));
        // link
        assert_eq!(v["values"][2], serde_json::json!("https://example.com/"));
        // description (trimmed)
        assert_eq!(v["values"][3], serde_json::json!("This is a <b>CDATA</b> description."));
    }

    #[test]
    fn writes_item_title_link_description_with_cdata() {
        // Arrange
        let out_dir = ensure_output_dir();

        let last_modified = "0";
        let etag = "[[NO_ETAG]]";
        let url = "https://example.com/feed.xml";
        let downloaded = "0";
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <title>Channel</title>
    <item>
      <title>Episode 1</title>
      <link>https://example.com/ep1</link>
      <description><![CDATA[ Hello & welcome! ]]></description>
    </item>
  </channel>
</rss>"#;

        let input = format!(
            "{last}\n{etag}\n{url}\n{dl}\n{xml}\n",
            last = last_modified,
            etag = etag,
            url = url,
            dl = downloaded,
            xml = xml
        );

        let feed_id = Some(777002_i64);

        // Act
        process_feed_sync(Cursor::new(input.into_bytes()), "<test>", feed_id);

        // Assert: find the nfitems file for this feed_id
        let entries = fs::read_dir(&out_dir).expect("output directory should be readable");
        let mut found_path: Option<PathBuf> = None;
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                if name.contains("_nfitems_") && name.ends_with("777002.json") {
                    found_path = Some(path);
                    break;
                }
            }
        }

        let file_path = found_path.expect("should have written an nfitems output file");
        let contents = fs::read_to_string(&file_path).expect("read nfitems file");
        let v: serde_json::Value = serde_json::from_str(&contents).expect("valid JSON");

        assert_eq!(v["table"], "nfitems");
        assert_eq!(v["values"][1], serde_json::json!("Episode 1"));
        assert_eq!(v["values"][2], serde_json::json!("https://example.com/ep1"));
        assert_eq!(v["values"][3], serde_json::json!("Hello & welcome!"));
    }

    #[test]
    fn writes_channel_itunes_author_to_newsfeeds_output() {
        // Arrange
        let out_dir = ensure_output_dir();

        let last_modified = "0";
        let etag = "[[NO_ETAG]]";
        let url = "https://example.com/feed.xml";
        let downloaded = "0";
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd">
  <channel>
    <title>Channel With iTunes</title>
    <itunes:author>  ACME Media  </itunes:author>
  </channel>
</rss>"#;

        let input = format!(
            "{last}\n{etag}\n{url}\n{dl}\n{xml}\n",
            last = last_modified,
            etag = etag,
            url = url,
            dl = downloaded,
            xml = xml
        );

        let feed_id = Some(808001_i64);

        // Act
        process_feed_sync(Cursor::new(input.into_bytes()), "<test>", feed_id);

        // Assert: find the newsfeeds file for this feed_id
        let entries = fs::read_dir(&out_dir).expect("output directory should be readable");
        let mut found_path: Option<PathBuf> = None;
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                if name.contains("_newsfeeds_") && name.ends_with("808001.json") {
                    found_path = Some(path);
                    break;
                }
            }
        }

        let file_path = found_path.expect("should have written a newsfeeds output file");
        let contents = fs::read_to_string(&file_path).expect("read newsfeeds file");
        let v: serde_json::Value = serde_json::from_str(&contents).expect("valid JSON");

        assert_eq!(v["table"], "newsfeeds");
        // itunes_author is the 6th value (index 5) and should be trimmed
        assert_eq!(v["values"][5], serde_json::json!("ACME Media"));
    }

    #[test]
    fn writes_item_itunes_author_to_nfitems_output() {
        // Arrange
        let out_dir = ensure_output_dir();

        let last_modified = "0";
        let etag = "[[NO_ETAG]]";
        let url = "https://example.com/feed.xml";
        let downloaded = "0";
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd">
  <channel>
    <title>Channel</title>
    <item>
      <title>Episode A</title>
      <itunes:author> Guest Speaker </itunes:author>
    </item>
  </channel>
</rss>"#;

        let input = format!(
            "{last}\n{etag}\n{url}\n{dl}\n{xml}\n",
            last = last_modified,
            etag = etag,
            url = url,
            dl = downloaded,
            xml = xml
        );

        let feed_id = Some(808002_i64);

        // Act
        process_feed_sync(Cursor::new(input.into_bytes()), "<test>", feed_id);

        // Assert: find the nfitems file for this feed_id
        let entries = fs::read_dir(&out_dir).expect("output directory should be readable");
        let mut found_path: Option<PathBuf> = None;
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                if name.contains("_nfitems_") && name.ends_with("808002.json") {
                    found_path = Some(path);
                    break;
                }
            }
        }

        let file_path = found_path.expect("should have written an nfitems output file");
        let contents = fs::read_to_string(&file_path).expect("read nfitems file");
        let v: serde_json::Value = serde_json::from_str(&contents).expect("valid JSON");

        assert_eq!(v["table"], "nfitems");
        // itunes_author is the 7th value (index 6) for nfitems
        assert_eq!(v["values"][6], serde_json::json!("Guest Speaker"));
    }

    #[test]
    fn writes_channel_generator_to_newsfeeds_output() {
        // Arrange
        let out_dir = ensure_output_dir();

        let last_modified = "0";
        let etag = "[[NO_ETAG]]";
        let url = "https://example.com/feed.xml";
        let downloaded = "0";
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <title>Channel Gen</title>
    <generator> WordPress 6.5.2 </generator>
  </channel>
</rss>"#;

        let input = format!(
            "{last}\n{etag}\n{url}\n{dl}\n{xml}\n",
            last = last_modified,
            etag = etag,
            url = url,
            dl = downloaded,
            xml = xml
        );

        let feed_id = Some(900901_i64);

        // Act
        process_feed_sync(Cursor::new(input.into_bytes()), "<test>", feed_id);

        // Assert: find the newsfeeds file for this feed_id
        let entries = fs::read_dir(&out_dir).expect("output directory should be readable");
        let mut found_path: Option<PathBuf> = None;
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                if name.contains("_newsfeeds_") && name.ends_with("900901.json") {
                    found_path = Some(path);
                    break;
                }
            }
        }

        let file_path = found_path.expect("should have written a newsfeeds output file");
        let contents = fs::read_to_string(&file_path).expect("read newsfeeds file");
        let v: serde_json::Value = serde_json::from_str(&contents).expect("valid JSON");

        assert_eq!(v["table"], "newsfeeds");
        // generator is the 5th value (index 4) and should be trimmed
        assert_eq!(v["values"][4], serde_json::json!("WordPress 6.5.2"));
    }
}