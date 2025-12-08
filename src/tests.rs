use super::*;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use chrono::{DateTime, Duration, Utc};
use serde_json::{json, Value as JsonValue};

fn unique_temp_dir() -> PathBuf {
    let base = std::env::temp_dir();
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    // Use high-resolution timestamp (nanoseconds) to minimize collision risk
    let dir = base.join(format!("feedparser_test_{}", ts));
    let _ = fs::create_dir_all(&dir);
    dir
}

fn ensure_output_dir() -> PathBuf {
    // Use get_or_init to atomically ensure only one directory is created
    // This prevents race conditions where parallel tests create different directories
    OUTPUT_SUBDIR.get_or_init(|| unique_temp_dir()).clone()
}

fn get_value(v: &serde_json::Value, col_name: &str) -> Option<serde_json::Value> {
    let columns = v["columns"].as_array()?;
    let values = v["values"].as_array()?;
    let mut targets = vec![col_name.to_string()];
    if col_name == "feed_id" {
        targets.push("id".to_string());
        targets.push("feedid".to_string());
    }
    if col_name == "pub_date" {
        targets.push("timestamp".to_string());
    }
    for (i, col) in columns.iter().enumerate() {
        if let Some(col_name) = col.as_str() {
            if targets.iter().any(|t| t == col_name) {
                return values.get(i).cloned();
            }
        }
    }
    None
}

fn sort_paths_by_numeric_prefix(paths: &mut Vec<PathBuf>) {
    paths.sort_by(|a, b| {
        let an = a.file_name().and_then(|s| s.to_str()).unwrap_or("");
        let bn = b.file_name().and_then(|s| s.to_str()).unwrap_or("");
        let anum = an.split('_').next().and_then(|s| s.parse::<u64>().ok());
        let bnum = bn.split('_').next().and_then(|s| s.parse::<u64>().ok());
        anum.cmp(&bnum).then_with(|| an.cmp(bn))
    });
}

fn read_json_file(path: &Path) -> serde_json::Value {
    let contents = fs::read_to_string(path).expect("read output file");
    serde_json::from_str(&contents).expect("valid JSON output")
}

fn output_files_for(out_dir: &Path, table: &str, feed_id: i64) -> Vec<PathBuf> {
    let suffix = format!("{feed_id}.json");
    let needle = format!("_{table}_");
    fs::read_dir(out_dir)
        .expect("output directory should be readable")
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_str()?.to_owned();
            if name.contains(&needle) && name.ends_with(&suffix) {
                Some(path)
            } else {
                None
            }
        })
        .collect()
}

fn output_records(out_dir: &Path, table: &str, feed_id: i64) -> Vec<serde_json::Value> {
    let mut files = output_files_for(out_dir, table, feed_id);
    sort_paths_by_numeric_prefix(&mut files);
    files.into_iter().map(|p| read_json_file(&p)).collect()
}

fn single_record(out_dir: &Path, table: &str, feed_id: i64) -> serde_json::Value {
    let mut records = output_records(out_dir, table, feed_id);
    assert_eq!(
        records.len(),
        1,
        "expected one {table} record for feed {feed_id}"
    );
    records.remove(0)
}

#[test]
fn test_newsfeeds_basic_channel_title() {
    let out_dir = ensure_output_dir();
    let feed = r#"0
[[NO_ETAG]]
https://example.com/feed.xml
0
<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
<title>My Test Channel</title>
  </channel>
</rss>"#;
    let feed_id = 424242_i64;
    process_feed_sync(Cursor::new(feed), "<test>", Some(feed_id));
    let v = single_record(&out_dir, "newsfeeds", feed_id);
    assert_eq!(v["table"], "newsfeeds");
    assert_eq!(v["feed_id"], serde_json::json!(424242));
    assert_eq!(get_value(&v, "title"), Some(serde_json::json!("My Test Channel")));
}

#[test]
fn test_newsfeeds_channel_link_and_description_cdata() {
    let out_dir = ensure_output_dir();
    let feed = r#"0
[[NO_ETAG]]
https://example.com/feed.xml
0
<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
<title>Channel With Links</title>
<link>https://example.com/</link>
<description><![CDATA[ This is a <b>CDATA</b> description. ]]></description>
  </channel>
</rss>"#;
    let feed_id = 777001_i64;
    process_feed_sync(Cursor::new(feed), "<test>", Some(feed_id));
    let v = single_record(&out_dir, "newsfeeds", feed_id);
    assert_eq!(v["table"], "newsfeeds");
    assert_eq!(get_value(&v, "title"), Some(serde_json::json!("Channel With Links")));
    assert_eq!(get_value(&v, "link"), Some(serde_json::json!("https://example.com/")));
    assert_eq!(get_value(&v, "description"), Some(serde_json::json!(" This is a <b>CDATA</b> description. ")));
}

#[test]
fn test_newsfeeds_html_entity_decoding() {
    let out_dir = ensure_output_dir();
    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd"
 xmlns:podcast="https://podcastindex.org/namespace/1.0">
<channel>
<title>HTML Entity Decoding</title>
<link>https://example.com</link>
<description>foo &copy; bar &ne; baz &#x1D306; qux</description>
<podcast:locked owner="foo &copy; bar &ne; baz &#x1D306; qux">yes</podcast:locked>
</channel>
</rss>"#;
    let feed_id = 2000_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));
    let v = single_record(&out_dir, "newsfeeds", feed_id);
    assert_eq!(get_value(&v, "description"), Some(JsonValue::from("foo © bar ≠ baz 𝌆 qux")));
    assert_eq!(get_value(&v, "podcast_locked"), Some(JsonValue::from(1)));
    assert_eq!(get_value(&v, "podcast_owner"), Some(JsonValue::from("foo © bar ≠ baz 𝌆 qux")));
}

#[test]
fn test_newsfeeds_complete_field_coverage() {
    let out_dir = ensure_output_dir();
    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd"
 xmlns:podcast="https://podcastindex.org/namespace/1.0"
 xmlns:atom="http://www.w3.org/2005/Atom">
<channel>
<title>Complete Channel</title>
<link>https://example.com</link>
<description>Full description</description>
<itunes:summary>Channel summary wins</itunes:summary>
<pubDate>Mon, 01 Jan 2024 12:00:00 GMT</pubDate>
<lastBuildDate>Mon, 01 Jan 2024 12:00:01 GMT</lastBuildDate>
<language>en-US</language>
<generator>TestGen 1.0</generator>
<itunes:author>Author Name</itunes:author>
<itunes:owner>
<itunes:name>Owner Name</itunes:name>
<itunes:email>owner@example.com</itunes:email>
</itunes:owner>
<itunes:category text="Technology">
<itunes:category text="Software"/>
</itunes:category>
<itunes:type>episodic</itunes:type>
<itunes:new-feed-url>https://new.example.com/feed.xml</itunes:new-feed-url>
<itunes:explicit>yes</itunes:explicit>
<itunes:image href="https://example.com/itunes.jpg"/>
<image><url>https://example.com/rss.jpg</url></image>
<podcast:guid>complete-guid</podcast:guid>
<podcast:locked owner="pod@example.com">yes</podcast:locked>
<podcast:funding url="https://example.com/support">Support us</podcast:funding>
<atom:link rel="hub" href="https://hub.example.com"/>
<atom:link rel="self" href="https://example.com/feed.xml"/>
<podcast:value type="lightning" method="keysend" suggested="5">
  <podcast:valueRecipient name="Alice" type="node" address="alice" split="90"/>
  <podcast:valueRecipient name="Bob" type="node" address="bob" split="10" fee="true"/>
</podcast:value>
<item>
<title>Item</title>
<guid>item1</guid>
<pubDate>Mon, 01 Jan 2024 12:00:00 GMT</pubDate>
<enclosure url="https://example.com/ep.mp3" length="123" type="audio/mpeg"/>
</item>
</channel>
</rss>"#;
    let feed_id = 2001_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));
    let v = single_record(&out_dir, "newsfeeds", feed_id);
    assert_eq!(get_value(&v, "feed_id"), Some(JsonValue::from(feed_id)));
    assert_eq!(get_value(&v, "title"), Some(JsonValue::from("Complete Channel")));
    assert_eq!(get_value(&v, "link"), Some(JsonValue::from("https://example.com")));
    assert_eq!(get_value(&v, "description"), Some(JsonValue::from("Channel summary wins")));
    assert_eq!(get_value(&v, "language"), Some(JsonValue::from("en-US")));
    assert_eq!(get_value(&v, "generator"), Some(JsonValue::from("TestGen 1.0")));
    assert_eq!(get_value(&v, "itunes_author"), Some(JsonValue::from("Author Name")));
    assert_eq!(get_value(&v, "itunes_owner_name"), Some(JsonValue::from("Owner Name")));
    assert_eq!(get_value(&v, "itunes_owner_email"), Some(JsonValue::from("owner@example.com")));
    assert_eq!(get_value(&v, "itunes_type"), Some(JsonValue::from("episodic")));
    assert_eq!(get_value(&v, "itunes_new_feed_url"), Some(JsonValue::from("https://new.example.com/feed.xml")));
    assert_eq!(get_value(&v, "explicit"), Some(JsonValue::from(1)));
    assert_eq!(get_value(&v, "podcast_locked"), Some(JsonValue::from(1)));
    assert_eq!(get_value(&v, "podcast_owner"), Some(JsonValue::from("pod@example.com")));
    assert_eq!(get_value(&v, "image"), Some(JsonValue::from("https://example.com/rss.jpg")));
    assert_eq!(get_value(&v, "itunes_image"), Some(JsonValue::from("https://example.com/itunes.jpg")));
    assert_eq!(get_value(&v, "itunes_categories"), Some(JsonValue::from(vec!["Technology".to_string(), "Software".to_string()])));
    assert_eq!(get_value(&v, "podcast_funding_url"), Some(JsonValue::from("https://example.com/support")));
    assert_eq!(get_value(&v, "podcast_funding_text"), Some(JsonValue::from("Support us")));
    assert_eq!(get_value(&v, "pubsub_hub_url"), Some(JsonValue::from("https://hub.example.com")));
    assert_eq!(get_value(&v, "pubsub_self_url"), Some(JsonValue::from("https://example.com/feed.xml")));
    assert_eq!(get_value(&v, "item_count"), Some(JsonValue::from(1)));
    let channel_value = get_value(&v, "podcast_value").unwrap();
    assert_eq!(channel_value["model"]["type"], "lightning");
    assert_eq!(channel_value["destinations"].as_array().unwrap().len(), 2);
    assert_eq!(channel_value["destinations"][1]["fee"], true);
}

#[test]
fn test_rss_pubsub_regular_links() {
    let out_dir = ensure_output_dir();
    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd"
 xmlns:podcast="https://podcastindex.org/namespace/1.0">
<channel>
<title>Pubsub RSS Feed</title>
<link>https://example.com</link>
<description>Test feed with pubsub in regular link elements</description>
<link rel="hub" href="https://pubsubhubbub.appspot.com/"/>
<link rel="self" href="https://example.com/feed.xml"/>
<item>
<title>Item</title>
<guid>item1</guid>
<pubDate>Mon, 01 Jan 2024 12:00:00 GMT</pubDate>
<enclosure url="https://example.com/ep.mp3" length="123" type="audio/mpeg"/>
</item>
</channel>
</rss>"#;
    let feed_id = 2004_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));
    let v = single_record(&out_dir, "newsfeeds", feed_id);
    assert_eq!(get_value(&v, "feed_id"), Some(JsonValue::from(feed_id)));
    assert_eq!(get_value(&v, "title"), Some(JsonValue::from("Pubsub RSS Feed")));
    assert_eq!(get_value(&v, "link"), Some(JsonValue::from("https://example.com")));
    assert_eq!(get_value(&v, "pubsub_hub_url"), Some(JsonValue::from("https://pubsubhubbub.appspot.com/")));
    assert_eq!(get_value(&v, "pubsub_self_url"), Some(JsonValue::from("https://example.com/feed.xml")));
}

#[test]
fn test_rss_pubsub_mixed_links() {
    let out_dir = ensure_output_dir();
    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd"
 xmlns:podcast="https://podcastindex.org/namespace/1.0"
 xmlns:atom="http://www.w3.org/2005/Atom">
<channel>
<title>Mixed Pubsub Links</title>
<link>https://example.com</link>
<description>Test feed with pubsub in both regular and atom:link elements</description>
<link rel="hub" href="https://hub1.example.com/"/>
<link rel="self" href="https://self1.example.com/feed.xml"/>
<atom:link rel="hub" href="https://hub2.example.com/"/>
<atom:link rel="self" href="https://self2.example.com/feed.xml"/>
<item>
<title>Item</title>
<guid>item1</guid>
<pubDate>Mon, 01 Jan 2024 12:00:00 GMT</pubDate>
<enclosure url="https://example.com/ep.mp3" length="123" type="audio/mpeg"/>
</item>
</channel>
</rss>"#;
    let feed_id = 2005_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));
    let v = single_record(&out_dir, "newsfeeds", feed_id);
    assert_eq!(get_value(&v, "feed_id"), Some(JsonValue::from(feed_id)));
    assert_eq!(get_value(&v, "title"), Some(JsonValue::from("Mixed Pubsub Links")));
    // Regular links should be processed first, so they should win
    assert_eq!(get_value(&v, "pubsub_hub_url"), Some(JsonValue::from("https://hub1.example.com/")));
    assert_eq!(get_value(&v, "pubsub_self_url"), Some(JsonValue::from("https://self1.example.com/feed.xml")));
}

#[test]
fn test_last_build_date_parsing() {
    let out_dir = ensure_output_dir();
    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd"
 xmlns:podcast="https://podcastindex.org/namespace/1.0">
<channel>
<title>LastBuildDate Test</title>
<link>https://example.com</link>
<description>Test feed with lastBuildDate</description>
<lastBuildDate>Mon, 01 Jan 2024 12:00:01 GMT</lastBuildDate>
<item>
<title>Item</title>
<guid>item1</guid>
<pubDate>Mon, 01 Jan 2024 12:00:00 GMT</pubDate>
<enclosure url="https://example.com/ep.mp3" length="123" type="audio/mpeg"/>
</item>
</channel>
</rss>"#;
    let feed_id = 2029_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));
    let v = single_record(&out_dir, "newsfeeds", feed_id);
    assert_eq!(get_value(&v, "feed_id"), Some(JsonValue::from(feed_id)));
    assert_eq!(get_value(&v, "title"), Some(JsonValue::from("LastBuildDate Test")));
    // lastBuildDate should be parsed and used as pub_date since pubDate is not present
    assert_eq!(get_value(&v, "pub_date"), Some(JsonValue::from(1704110401))); // Mon, 01 Jan 2024 12:00:01 GMT
    // lastBuildDate should also be present as a separate field
    assert_eq!(get_value(&v, "last_build_date"), Some(JsonValue::from(1704110401))); // Mon, 01 Jan 2024 12:00:01 GMT
}

#[test]
fn test_last_build_date_missing() {
    let out_dir = ensure_output_dir();
    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd"
 xmlns:podcast="https://podcastindex.org/namespace/1.0">
<channel>
<title>No LastBuildDate Test</title>
<link>https://example.com</link>
<description>Test feed without lastBuildDate</description>
<pubDate>Mon, 01 Jan 2024 12:00:00 GMT</pubDate>
<item>
<title>Item</title>
<guid>item1</guid>
<pubDate>Mon, 01 Jan 2024 12:00:00 GMT</pubDate>
<enclosure url="https://example.com/ep.mp3" length="123" type="audio/mpeg"/>
</item>
</channel>
</rss>"#;
    let feed_id = 2031_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));
    let v = single_record(&out_dir, "newsfeeds", feed_id);
    assert_eq!(get_value(&v, "feed_id"), Some(JsonValue::from(feed_id)));
    assert_eq!(get_value(&v, "title"), Some(JsonValue::from("No LastBuildDate Test")));
    // pubDate should be present
    assert_eq!(get_value(&v, "pub_date"), Some(JsonValue::from(1704110400))); // Mon, 01 Jan 2024 12:00:00 GMT
    // lastBuildDate should be 0 when not present
    assert_eq!(get_value(&v, "last_build_date"), Some(JsonValue::from(0)));
}

#[test]
fn test_last_build_date_fallback() {
    let out_dir = ensure_output_dir();
    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd"
 xmlns:podcast="https://podcastindex.org/namespace/1.0">
<channel>
<title>LastBuildDate Fallback Test</title>
<link>https://example.com</link>
<description>Test feed with both pubDate and lastBuildDate</description>
<pubDate>Mon, 01 Jan 2024 12:00:00 GMT</pubDate>
<lastBuildDate>Mon, 01 Jan 2024 12:00:01 GMT</lastBuildDate>
<item>
<title>Item</title>
<guid>item1</guid>
<pubDate>Mon, 01 Jan 2024 12:00:00 GMT</pubDate>
<enclosure url="https://example.com/ep.mp3" length="123" type="audio/mpeg"/>
</item>
</channel>
</rss>"#;
    let feed_id = 2030_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));
    let v = single_record(&out_dir, "newsfeeds", feed_id);
    assert_eq!(get_value(&v, "feed_id"), Some(JsonValue::from(feed_id)));
    assert_eq!(get_value(&v, "title"), Some(JsonValue::from("LastBuildDate Fallback Test")));
    // pubDate should be used when present, not lastBuildDate
    assert_eq!(get_value(&v, "pub_date"), Some(JsonValue::from(1704110400))); // Mon, 01 Jan 2024 12:00:00 GMT
    // lastBuildDate should still be present as a separate field
    assert_eq!(get_value(&v, "last_build_date"), Some(JsonValue::from(1704110401))); // Mon, 01 Jan 2024 12:00:01 GMT
}

#[test]
fn test_last_build_date_only() {
    let out_dir = ensure_output_dir();
    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd"
 xmlns:podcast="https://podcastindex.org/namespace/1.0">
<channel>
<title>LastBuildDate Only Test</title>
<link>https://example.com</link>
<description>Test feed with only lastBuildDate, no pubDate</description>
<lastBuildDate>Tue, 02 Jan 2024 15:30:45 GMT</lastBuildDate>
<item>
<title>Item</title>
<guid>item1</guid>
<pubDate>Mon, 01 Jan 2024 12:00:00 GMT</pubDate>
<enclosure url="https://example.com/ep.mp3" length="123" type="audio/mpeg"/>
</item>
</channel>
</rss>"#;
    let feed_id = 2032_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));
    let v = single_record(&out_dir, "newsfeeds", feed_id);
    assert_eq!(get_value(&v, "feed_id"), Some(JsonValue::from(feed_id)));
    assert_eq!(get_value(&v, "title"), Some(JsonValue::from("LastBuildDate Only Test")));
    // lastBuildDate should be used as fallback when pubDate is missing
    // Use the actual parsed timestamp value
    let expected_timestamp = utils::pub_date_to_timestamp("Tue, 02 Jan 2024 15:30:45 GMT");
    assert_eq!(get_value(&v, "pub_date"), Some(JsonValue::from(expected_timestamp)));
    // lastBuildDate should also be present as a separate field
    assert_eq!(get_value(&v, "last_build_date"), Some(JsonValue::from(expected_timestamp)));
}

#[test]
fn test_channel_pubdate_fallback_to_newest_item() {
    let out_dir = ensure_output_dir();
    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd"
 xmlns:podcast="https://podcastindex.org/namespace/1.0">
<channel>
<title>NewestItemPubDate Fallback Test</title>
<link>https://example.com</link>
<description>Test feed with no pubDate or lastBuildDate, should use newest item pubDate</description>
<item>
<title>Item 1</title>
<guid>item1</guid>
<pubDate>Mon, 01 Jan 2024 12:00:00 GMT</pubDate>
<enclosure url="https://example.com/ep1.mp3" length="123" type="audio/mpeg"/>
</item>
<item>
<title>Item 2</title>
<guid>item2</guid>
<pubDate>Tue, 02 Jan 2024 15:30:45 GMT</pubDate>
<enclosure url="https://example.com/ep2.mp3" length="456" type="audio/mpeg"/>
</item>
    </channel>
</rss>"#;
    let feed_id = 2036_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));
    let v = single_record(&out_dir, "newsfeeds", feed_id);
    assert_eq!(get_value(&v, "feed_id"), Some(JsonValue::from(feed_id)));
    assert_eq!(get_value(&v, "title"), Some(JsonValue::from("NewestItemPubDate Fallback Test")));
    // pubDate should fallback to newestItemPubDate when both pubDate and lastBuildDate are 0
    let expected_newest = utils::pub_date_to_timestamp("Tue, 02 Jan 2024 15:30:45 GMT");
    assert_eq!(get_value(&v, "pub_date"), Some(JsonValue::from(expected_newest)));
    assert_eq!(get_value(&v, "newest_item_pub_date"), Some(JsonValue::from(expected_newest)));
    assert_eq!(get_value(&v, "last_build_date"), Some(JsonValue::from(0)));
}

#[test]
fn test_channel_pubdate_priority_order() {
    let out_dir = ensure_output_dir();
    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd"
 xmlns:podcast="https://podcastindex.org/namespace/1.0">
<channel>
<title>PubDate Priority Test</title>
<link>https://example.com</link>
<description>Test that pubDate takes priority over lastBuildDate and newestItemPubDate</description>
<pubDate>Mon, 01 Jan 2024 10:00:00 GMT</pubDate>
<lastBuildDate>Mon, 01 Jan 2024 11:00:00 GMT</lastBuildDate>
<item>
<title>Item 1</title>
<guid>item1</guid>
<pubDate>Tue, 02 Jan 2024 15:30:45 GMT</pubDate>
<enclosure url="https://example.com/ep1.mp3" length="123" type="audio/mpeg"/>
</item>
</channel>
</rss>"#;
    let feed_id = 2034_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));
    let v = single_record(&out_dir, "newsfeeds", feed_id);
    assert_eq!(get_value(&v, "feed_id"), Some(JsonValue::from(feed_id)));
    assert_eq!(get_value(&v, "title"), Some(JsonValue::from("PubDate Priority Test")));
    // pubDate should be used (not lastBuildDate or newestItemPubDate)
    let expected_pubdate = utils::pub_date_to_timestamp("Mon, 01 Jan 2024 10:00:00 GMT");
    assert_eq!(get_value(&v, "pub_date"), Some(JsonValue::from(expected_pubdate)));
    // lastBuildDate should still be present
    let expected_lastbuild = utils::pub_date_to_timestamp("Mon, 01 Jan 2024 11:00:00 GMT");
    assert_eq!(get_value(&v, "last_build_date"), Some(JsonValue::from(expected_lastbuild)));
    // newestItemPubDate should be from the item
    let expected_newest = utils::pub_date_to_timestamp("Tue, 02 Jan 2024 15:30:45 GMT");
    assert_eq!(get_value(&v, "newest_item_pub_date"), Some(JsonValue::from(expected_newest)));
}

#[test]
fn test_channel_pubdate_lastbuilddate_fallback() {
    let out_dir = ensure_output_dir();
    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd"
 xmlns:podcast="https://podcastindex.org/namespace/1.0">
<channel>
<title>LastBuildDate Fallback Test</title>
<link>https://example.com</link>
<description>Test that lastBuildDate is used when pubDate is 0</description>
<lastBuildDate>Mon, 01 Jan 2024 11:00:00 GMT</lastBuildDate>
<item>
<title>Item 1</title>
<guid>item1</guid>
<pubDate>Tue, 02 Jan 2024 15:30:45 GMT</pubDate>
<enclosure url="https://example.com/ep1.mp3" length="123" type="audio/mpeg"/>
</item>
</channel>
</rss>"#;
    let feed_id = 2035_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));
    let v = single_record(&out_dir, "newsfeeds", feed_id);
    assert_eq!(get_value(&v, "feed_id"), Some(JsonValue::from(feed_id)));
    assert_eq!(get_value(&v, "title"), Some(JsonValue::from("LastBuildDate Fallback Test")));
    // pubDate should fallback to lastBuildDate (not newestItemPubDate, since lastBuildDate exists)
    let expected_lastbuild = utils::pub_date_to_timestamp("Mon, 01 Jan 2024 11:00:00 GMT");
    assert_eq!(get_value(&v, "pub_date"), Some(JsonValue::from(expected_lastbuild)));
    assert_eq!(get_value(&v, "last_build_date"), Some(JsonValue::from(expected_lastbuild)));
}

#[test]
fn test_newsfeeds_hashes_and_counts() {
    let out_dir = ensure_output_dir();
    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd"
 xmlns:podcast="https://podcastindex.org/namespace/1.0">
<channel>
<title>Hash Channel</title>
<link>https://hash.example.com</link>
<description>Hashing</description>
<language>en</language>
<generator>HashGen</generator>
<itunes:author>Hash Author</itunes:author>
<itunes:owner>
<itunes:name>Hash Owner</itunes:name>
<itunes:email>hash@example.com</itunes:email>
</itunes:owner>
<itunes:explicit>no</itunes:explicit>

<item>
<title>First</title>
<itunes:title>First IT</itunes:title>
<link>https://hash.example.com/1</link>
<pubDate>Mon, 01 Jan 2024 12:00:00 GMT</pubDate>
<guid>g1</guid>
<enclosure url="https://hash.example.com/1.mp3" length="10" type="audio/mpeg"/>
<podcast:funding url="https://fund.example.com/1">Fund1</podcast:funding>
</item>
<item>
<title>Second</title>
<itunes:title>Second IT</itunes:title>
<link>https://hash.example.com/2</link>
<pubDate>Tue, 02 Jan 2024 12:00:00 GMT</pubDate>
<guid>g2</guid>
<enclosure url="https://hash.example.com/2.mp3" length="20" type="audio/mpeg"/>
<podcast:funding url="https://fund.example.com/2">Fund2</podcast:funding>
</item>
</channel>
</rss>"#;
    let feed_id = 20100_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));
    let v = single_record(&out_dir, "newsfeeds", feed_id);
    let newest = utils::pub_date_to_timestamp("Tue, 02 Jan 2024 12:00:00 GMT");
    let oldest = utils::pub_date_to_timestamp("Mon, 01 Jan 2024 12:00:00 GMT");
    assert_eq!(get_value(&v, "item_count"), Some(JsonValue::from(2)));
    assert_eq!(get_value(&v, "newest_item_pub_date"), Some(JsonValue::from(newest)));
    assert_eq!(get_value(&v, "oldest_item_pub_date"), Some(JsonValue::from(oldest)));
}

#[test]
fn test_newsfeeds_channel_value_lightning_priority() {
    let out_dir = ensure_output_dir();
    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss xmlns:podcast="https://podcastindex.org/namespace/1.0">
<channel>
<title>Value Priority</title>
<podcast:value type="bitcoin" method="custom">
<podcast:valueRecipient name="BTC" type="node" address="btc" split="100"/>
</podcast:value>
<podcast:value type="lightning" method="keysend">
<podcast:valueRecipient name="LN" type="node" address="ln" split="100"/>
</podcast:value>
</channel>
</rss>"#;
    let feed_id = 2018_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));
    let v = single_record(&out_dir, "newsfeeds", feed_id);
    let vb = get_value(&v, "podcast_value").unwrap();
    assert_eq!(vb["model"]["type"], "lightning");
    assert_eq!(vb["destinations"][0]["name"], "LN");
}

#[test]
fn test_newsfeeds_channel_value_fallback_no_lightning() {
    let out_dir = ensure_output_dir();
    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss xmlns:podcast="https://podcastindex.org/namespace/1.0">
<channel>
<title>Value Fallback</title>
<podcast:value type="bitcoin" method="custom">
<podcast:valueRecipient name="BTC" type="node" address="btc" split="100"/>
</podcast:value>
<podcast:value type="HBD" method="keysend">
<podcast:valueRecipient name="HBD" type="node" address="hbd" split="100"/>
</podcast:value>
</channel>
</rss>"#;
    let feed_id = 20194_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));
    let v = single_record(&out_dir, "newsfeeds", feed_id);
    let vb = get_value(&v, "podcast_value").unwrap();
    assert_eq!(vb["model"]["type"], "bitcoin");
    assert_eq!(vb["destinations"][0]["name"], "BTC");
}

#[test]
fn test_newsfeeds_locked_owner_email_fallback() {
    let out_dir = ensure_output_dir();
    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd" xmlns:podcast="https://podcastindex.org/namespace/1.0">
<channel>
<title>Owner Fallback</title>
<itunes:owner>
<itunes:email>owner@example.com</itunes:email>
</itunes:owner>
<podcast:locked>yes</podcast:locked>
<item>
<title>Episode</title>
<guid>ep</guid>
<enclosure url="https://example.com/ep.mp3" length="1" type="audio/mpeg"/>
</item>
</channel>
</rss>"#;
    let feed_id = 2025_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));
    let v = single_record(&out_dir, "newsfeeds", feed_id);
    assert_eq!(get_value(&v, "podcast_locked"), Some(JsonValue::from(1)));
    assert_eq!(get_value(&v, "podcast_owner"), Some(JsonValue::from("owner@example.com")));
}

#[test]
fn test_newsfeeds_update_frequency_and_epoch_pubdates() {
    let out_dir = ensure_output_dir();
    let now = Utc::now();
    let recent = now - Duration::days(1);
    let rfc_now = DateTime::<Utc>::from(now).to_rfc2822();
    let rfc_recent = DateTime::<Utc>::from(recent).to_rfc2822();
    let feed = format!(r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss>
<channel>
<title>UpdateFreq</title>
<item>
<title>Ep1</title>
<guid>g1</guid>
<pubDate>{}</pubDate>
<enclosure url="https://example.com/ep1.mp3" length="10" type="audio/mpeg"/>
</item>
<item>
<title>Ep2</title>
<guid>g2</guid>
<pubDate>{}</pubDate>
<enclosure url="https://example.com/ep2.mp3" length="20" type="audio/mpeg"/>
</item>
</channel>
</rss>"#, rfc_now, rfc_recent);
    let feed_id = 30303_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));
    let v = single_record(&out_dir, "newsfeeds", feed_id);
    assert_eq!(get_value(&v, "item_count"), Some(JsonValue::from(2)));
    assert_eq!(get_value(&v, "newest_item_pub_date"), Some(JsonValue::from(now.timestamp())));
    assert_eq!(get_value(&v, "oldest_item_pub_date"), Some(JsonValue::from(recent.timestamp())));
    assert_eq!(get_value(&v, "update_frequency"), Some(JsonValue::from(1)));
}

#[test]
fn test_newsfeeds_itunes_metadata_fields() {
    let out_dir = ensure_output_dir();
    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd">
<channel>
<title>Meta Channel</title>
<generator>GenX/1.2</generator>
<itunes:type>trailer</itunes:type>
<itunes:new-feed-url>https://example.com/new.xml</itunes:new-feed-url>
<itunes:owner><itunes:email>meta@example.com</itunes:email></itunes:owner>
<itunes:image href="https://example.com/meta.jpg"/>
<item>
<title>Ep</title>
<guid>g-meta</guid>
<enclosure url="https://example.com/ep.mp3" length="1" type="audio/mpeg"/>
</item>
</channel>
</rss>"#;
    let feed_id = 33001_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));
    let v = single_record(&out_dir, "newsfeeds", feed_id);
    assert_eq!(get_value(&v, "generator"), Some(JsonValue::from("GenX/1.2")));
    assert_eq!(get_value(&v, "itunes_type"), Some(JsonValue::from("trailer")));
    assert_eq!(get_value(&v, "itunes_new_feed_url"), Some(JsonValue::from("https://example.com/new.xml")));
}

#[test]
fn test_newsfeeds_locked_owner_attribute() {
    let out_dir = ensure_output_dir();
    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd" xmlns:podcast="https://podcastindex.org/namespace/1.0">
<channel>
<title>Locked Owner</title>
<itunes:owner><itunes:email>fallback@example.com</itunes:email></itunes:owner>
<podcast:locked owner="owner@example.com">yes</podcast:locked>
<item>
<title>Ep</title>
<guid>g-lock</guid>
<enclosure url="https://example.com/ep.mp3" length="1" type="audio/mpeg"/>
</item>
</channel>
</rss>"#;
    let feed_id = 33002_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));
    let v = single_record(&out_dir, "newsfeeds", feed_id);
    assert_eq!(get_value(&v, "podcast_locked"), Some(JsonValue::from(1)));
    assert_eq!(get_value(&v, "podcast_owner"), Some(JsonValue::from("owner@example.com")));
}

#[test]
fn test_nfitems_basic_item_with_cdata() {
    let out_dir = ensure_output_dir();
    let feed = r#"0
[[NO_ETAG]]
https://example.com/feed.xml
0
<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
<title>Channel</title>
<item>
  <title>Episode 1</title>
  <link>https://example.com/ep1</link>
  <description><![CDATA[ Hello & welcome! ]]></description>
  <enclosure url="https://example.com/ep1.mp3" length="1234" type="audio/mpeg"/>
</item>
  </channel>
</rss>"#;
    let feed_id = 777002_i64;
    process_feed_sync(Cursor::new(feed), "<test>", Some(feed_id));
    let v = single_record(&out_dir, "nfitems", feed_id);
    assert_eq!(v["table"], "nfitems");
    assert_eq!(v["values"][1], serde_json::json!("Episode 1"));
    assert_eq!(v["values"][2], serde_json::json!("https://example.com/ep1"));
    assert_eq!(v["values"][3], serde_json::json!("Hello & welcome!"));
}

#[test]
fn test_nfitems_complete_field_coverage() {
    let out_dir = ensure_output_dir();
    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd"
 xmlns:podcast="https://podcastindex.org/namespace/1.0"
 xmlns:content="http://purl.org/rss/1.0/modules/content/">
<channel>
<title>Test Feed</title>
<item>
<title>Complete Episode</title>
<itunes:title>Itunes Episode Title</itunes:title>
<link>https://example.com/ep1</link>
<description>Episode description</description>
<itunes:summary>Itunes summary wins</itunes:summary>
<content:encoded>Content encoded fallback</content:encoded>
<pubDate>Mon, 01 Jan 2024 12:00:00 GMT</pubDate>
<guid>ep-guid</guid>
<itunes:image href="https://example.com/ep.jpg"/>
<itunes:duration>1:05</itunes:duration>
<itunes:episode>42</itunes:episode>
<itunes:season>3</itunes:season>
<itunes:episodeType>full</itunes:episodeType>
<itunes:explicit>yes</itunes:explicit>
<enclosure url="https://example.com/ep.mp3" length="12345678" type="audio/mpeg"/>
<podcast:funding url="https://donate.example.com">Support!</podcast:funding>
<podcast:transcript url="https://example.com/ep.vtt" type="text/vtt"/>
<podcast:chapters url="https://example.com/chapters.json" type="application/json"/>
<podcast:soundbite startTime="10" duration="15">Clip</podcast:soundbite>
<podcast:person role="host" group="cast" img="https://example.com/host.jpg" href="https://example.com/host">Host Name</podcast:person>
<podcast:value type="lightning" method="keysend" suggested="1">
  <podcast:valueRecipient name="Podcaster" type="node" address="addr" split="100"/>
</podcast:value>
</item>
</channel>
</rss>"#;
    let feed_id = 2002_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));
    let mut items = output_records(&out_dir, "nfitems", feed_id);
    assert_eq!(items.len(), 1);
    let item = items.pop().unwrap();
    assert_eq!(get_value(&item, "title"), Some(JsonValue::from("Itunes Episode Title")));
    assert_eq!(get_value(&item, "link"), Some(JsonValue::from("https://example.com/ep1")));
    assert_eq!(get_value(&item, "description"), Some(JsonValue::from("Content encoded fallback")));
    let expected_pub_date = utils::pub_date_to_timestamp("Mon, 01 Jan 2024 12:00:00 GMT");
    assert_eq!(get_value(&item, "pub_date"), Some(JsonValue::from(expected_pub_date)));
    assert_eq!(get_value(&item, "guid"), Some(JsonValue::from("ep-guid")));
    assert_eq!(get_value(&item, "image"), Some(JsonValue::from("https://example.com/ep.jpg")));
    assert_eq!(get_value(&item, "itunes_duration"), Some(JsonValue::from(65)));
    assert_eq!(get_value(&item, "itunes_episode"), Some(JsonValue::from(42)));
    assert_eq!(get_value(&item, "itunes_season"), Some(JsonValue::from(3)));
    assert_eq!(get_value(&item, "itunes_episode_type"), Some(JsonValue::from("full")));
    assert_eq!(get_value(&item, "itunes_explicit"), Some(JsonValue::from(1)));
}

#[test]
fn test_episode_number_non_digit_stripping() {
    let out_dir = ensure_output_dir();
    let feed = r#"0
[[NO_ETAG]]
https://example.com/feed.xml
0
<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd">
  <channel>
    <title>Episode Number Stripping Test</title>
    <item>
      <title>Episode with text</title>
      <guid>ep1</guid>
      <itunes:episode>Episode 42</itunes:episode>
      <enclosure url="https://example.com/ep1.mp3" length="123" type="audio/mpeg"/>
    </item>
    <item>
      <title>Episode with hash</title>
      <guid>ep2</guid>
      <itunes:episode>#123</itunes:episode>
      <enclosure url="https://example.com/ep2.mp3" length="123" type="audio/mpeg"/>
    </item>
    <item>
      <title>Episode with season format</title>
      <guid>ep3</guid>
      <itunes:episode>S01E05</itunes:episode>
      <enclosure url="https://example.com/ep3.mp3" length="123" type="audio/mpeg"/>
    </item>
    <item>
      <title>Episode with decimal</title>
      <guid>ep4</guid>
      <itunes:episode>42.5</itunes:episode>
      <enclosure url="https://example.com/ep4.mp3" length="123" type="audio/mpeg"/>
    </item>
    <item>
      <title>Episode with only non-digits</title>
      <guid>ep5</guid>
      <itunes:episode>No numbers here</itunes:episode>
      <enclosure url="https://example.com/ep5.mp3" length="123" type="audio/mpeg"/>
    </item>
    <item>
      <title>Episode with mixed</title>
      <guid>ep6</guid>
      <itunes:episode>Ep 999 Special</itunes:episode>
      <enclosure url="https://example.com/ep6.mp3" length="123" type="audio/mpeg"/>
    </item>
  </channel>
</rss>"#;
    let feed_id = 33009_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));

    // Test Episode 42 -> 42
    let items = output_records(&out_dir, "nfitems", feed_id);
    let ep1 = items.iter().find(|i| get_value(i, "title") == Some(JsonValue::from("Episode with text"))).unwrap();
    assert_eq!(get_value(ep1, "itunes_episode"), Some(JsonValue::from(42)));

    // Test #123 -> 123
    let ep2 = items.iter().find(|i| get_value(i, "title") == Some(JsonValue::from("Episode with hash"))).unwrap();
    assert_eq!(get_value(ep2, "itunes_episode"), Some(JsonValue::from(123)));

    // Test S01E05 -> 0105 -> 105
    let ep3 = items.iter().find(|i| get_value(i, "title") == Some(JsonValue::from("Episode with season format"))).unwrap();
    assert_eq!(get_value(ep3, "itunes_episode"), Some(JsonValue::from(105)));

    // Test 42.5 -> 425
    let ep4 = items.iter().find(|i| get_value(i, "title") == Some(JsonValue::from("Episode with decimal"))).unwrap();
    assert_eq!(get_value(ep4, "itunes_episode"), Some(JsonValue::from(425)));

    // Test "No numbers here" -> empty -> null
    let ep5 = items.iter().find(|i| get_value(i, "title") == Some(JsonValue::from("Episode with only non-digits"))).unwrap();
    assert_eq!(get_value(ep5, "itunes_episode"), Some(JsonValue::Null));

    // Test "Ep 999 Special" -> 999
    let ep6 = items.iter().find(|i| get_value(i, "title") == Some(JsonValue::from("Episode with mixed"))).unwrap();
    assert_eq!(get_value(ep6, "itunes_episode"), Some(JsonValue::from(999)));
}

#[test]
fn test_nfitems_transcripts_with_type_detection() {
    let out_dir = ensure_output_dir();
    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss xmlns:podcast="https://podcastindex.org/namespace/1.0">
<channel>
<title>Transcript Test</title>
<item>
<title>Ep 1</title>
<guid>ep1</guid>
<enclosure url="https://example.com/ep1.mp3" length="123" type="audio/mpeg"/>
<podcast:transcript url="https://example.com/t1.json" type="application/json"/>
</item>
<item>
<title>Ep 2</title>
<guid>ep2</guid>
<enclosure url="https://example.com/ep2.mp3" length="123" type="audio/mpeg"/>
<podcast:transcript url="https://example.com/t2.srt" type="text/srt"/>
</item>
<item>
<title>Ep 3</title>
<guid>ep3</guid>
<enclosure url="https://example.com/ep3.mp3" length="123" type="audio/mpeg"/>
<podcast:transcript url="https://example.com/t3.vtt" type="text/vtt"/>
</item>
</channel>
</rss>"#;
    let feed_id = 2006_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));
    let nfitems = output_records(&out_dir, "nfitems", feed_id);
    assert_eq!(nfitems.len(), 3);
    let mut types = Vec::new();
    for item in &nfitems {
        if let Some(transcripts_val) = get_value(item, "podcast_transcripts") {
            if let Some(transcripts_array) = transcripts_val.as_array() {
                for transcript in transcripts_array {
                    if let Some(type_val) = transcript.get("type") {
                        if let Some(t) = type_val.as_str() {
                            types.push(t.to_string());
                        }
                    }
                }
            }
        }
    }
    assert!(types.iter().any(|t| t.contains("json")), "Should have JSON type");
    assert!(types.iter().any(|t| t.contains("srt")), "Should have SRT type");
    assert!(types.iter().any(|t| t.contains("vtt")), "Should have VTT type");
}

#[test]
fn test_nfitems_chapters() {
    let out_dir = ensure_output_dir();
    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss xmlns:podcast="https://podcastindex.org/namespace/1.0">
<channel>
<title>Chapters Test</title>
<item>
<title>Episode</title>
<guid>ep</guid>
<enclosure url="https://example.com/ep.mp3" length="123" type="audio/mpeg"/>
<podcast:chapters url="https://example.com/chapters.json" type="application/json"/>
</item>
</channel>
</rss>"#;
    let feed_id = 2007_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));
    let mut nfitems = output_records(&out_dir, "nfitems", feed_id);
    assert_eq!(nfitems.len(), 1);
    let item = nfitems.pop().unwrap();
    let chapters_val = get_value(&item, "podcast_chapters").unwrap();
    let chapters_array = chapters_val.as_array().unwrap();
    assert_eq!(chapters_array.len(), 1);
    let chapter = &chapters_array[0];
    assert_eq!(chapter.get("url"), Some(&JsonValue::from("https://example.com/chapters.json")));
}

#[test]
fn test_nfitems_soundbites() {
    let out_dir = ensure_output_dir();
    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss xmlns:podcast="https://podcastindex.org/namespace/1.0">
<channel>
<title>Soundbites Test</title>
<item>
<title>Episode</title>
<guid>ep</guid>
<enclosure url="https://example.com/ep.mp3" length="123" type="audio/mpeg"/>
<podcast:soundbite startTime="10" duration="30">Intro</podcast:soundbite>
<podcast:soundbite startTime="100" duration="45">Main topic</podcast:soundbite>
</item>
</channel>
</rss>"#;
    let feed_id = 2008_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));
    let nfitems = output_records(&out_dir, "nfitems", feed_id);
    assert_eq!(nfitems.len(), 1);
    let item = &nfitems[0];
    let soundbites_val = get_value(item, "podcast_soundbites").unwrap();
    let soundbites_array = soundbites_val.as_array().unwrap();
    assert_eq!(soundbites_array.len(), 2);
    assert_eq!(soundbites_array[0].get("title"), Some(&JsonValue::from("Intro")));
    assert_eq!(soundbites_array[0].get("start"), Some(&JsonValue::from("10")));
    assert_eq!(soundbites_array[1].get("title"), Some(&JsonValue::from("Main topic")));
}

#[test]
fn test_nfitems_persons() {
    let out_dir = ensure_output_dir();
    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss xmlns:podcast="https://podcastindex.org/namespace/1.0">
<channel>
<title>Persons Test</title>
<item>
<title>Episode</title>
<guid>ep</guid>
<enclosure url="https://example.com/ep.mp3" length="123" type="audio/mpeg"/>
<podcast:person role="host" group="cast" img="https://example.com/host.jpg" href="https://example.com/host">Alice</podcast:person>
<podcast:person role="guest">Bob</podcast:person>
</item>
</channel>
</rss>"#;
    let feed_id = 2009_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));
    let nfitems = output_records(&out_dir, "nfitems", feed_id);
    assert_eq!(nfitems.len(), 1);
    let item = &nfitems[0];
    let persons_val = get_value(item, "podcast_persons").unwrap();
    let persons_array = persons_val.as_array().unwrap();
    assert_eq!(persons_array.len(), 2);
    assert_eq!(persons_array[0].get("name"), Some(&JsonValue::from("Alice")));
    assert_eq!(persons_array[0].get("role"), Some(&JsonValue::from("host")));
    assert_eq!(persons_array[1].get("name"), Some(&JsonValue::from("Bob")));
    assert_eq!(persons_array[1].get("role"), Some(&JsonValue::from("guest")));
}

#[test]
fn test_nfitems_value() {
    let out_dir = ensure_output_dir();
    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss xmlns:podcast="https://podcastindex.org/namespace/1.0">
<channel>
<title>Item Value Test</title>
<item>
<title>Episode</title>
<guid>ep</guid>
<enclosure url="https://example.com/ep.mp3" length="123" type="audio/mpeg"/>
<podcast:value type="lightning" method="keysend" suggested="0.00000005000">
    <podcast:valueRecipient name="Podcaster" type="node" address="addr123" split="90"/>
    <podcast:valueRecipient name="App" type="node" address="addr456" split="10" fee="true"/>
</podcast:value>
</item>
</channel>
</rss>"#;
    let feed_id = 2010_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));
    let nfitems = output_records(&out_dir, "nfitems", feed_id);
    assert_eq!(nfitems.len(), 1);
    let item = &nfitems[0];
    let value = get_value(item, "podcast_values").unwrap();
    assert_eq!(value["model"]["type"], "lightning");
    assert_eq!(value["destinations"].as_array().unwrap().len(), 2);
}

// Edge case: Empty feed
#[test]
fn test_empty_feed() {
    // Arrange
    let out_dir = ensure_output_dir();

    let feed = r#"1
[[NO_ETAG]]
https://www.ualrpublicradio.org/podcast/arts-letters/rss.xml
1745569945
"#;

    // Act
    let feed_id = 1337_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));

    // Assert: find the newsfeeds file for this feed_id
    let v = single_record(&out_dir, "newsfeeds", feed_id);

    assert_eq!(v["table"], "newsfeeds");
    assert_eq!(v["feed_id"], serde_json::json!(feed_id));
    assert_eq!(get_value(&v, "title"), Some(serde_json::json!("")));
    assert_eq!(get_value(&v, "link"), Some(serde_json::json!("")));
    assert_eq!(get_value(&v, "description"), Some(serde_json::json!("")));
}




// Edge case: Image fallback (itunes:image when regular image is empty)
#[test]
fn test_image_fallback() {
    let out_dir = ensure_output_dir();

    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd">
<channel>
<title>Image Fallback</title>
<itunes:image href="https://example.com/itunes-only.jpg"/>
<item>
<title>Episode</title>
<guid>ep</guid>
<enclosure url="https://example.com/ep.mp3" length="123" type="audio/mpeg"/>
<itunes:image href="https://example.com/ep-itunes-only.jpg"/>
</item>
</channel>
</rss>"#;

    let feed_id = 2014_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));

    let nf = single_record(&out_dir, "newsfeeds", feed_id);
    let nfitems = output_records(&out_dir, "nfitems", feed_id);

    assert_eq!(get_value(&nf, "image"), Some(JsonValue::from("https://example.com/itunes-only.jpg")));

    assert_eq!(nfitems.len(), 1);
    let item = &nfitems[0];

    assert_eq!(get_value(item, "image"), Some(JsonValue::from("https://example.com/ep-itunes-only.jpg")));
}

#[test]
fn test_episode_season_parsing() {
    let out_dir = ensure_output_dir();

    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd">
<channel>
<title>Parsing Test</title>
<item>
<title>Episode</title>
<guid>ep</guid>
<enclosure url="https://example.com/ep.mp3" length="123" type="audio/mpeg"/>
<itunes:episode>10</itunes:episode>
<itunes:season>02</itunes:season>
</item>
</channel>
</rss>"#;

    let feed_id = 2016_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));

    let mut nfitems = output_records(&out_dir, "nfitems", feed_id);
    assert_eq!(nfitems.len(), 1);
    let item = nfitems.pop().unwrap();

    assert_eq!(get_value(&item, "itunes_episode"), Some(JsonValue::from(10)));
    assert_eq!(get_value(&item, "itunes_season"), Some(JsonValue::from(2)));
}

// Edge case: Multiple items
#[test]
fn test_multiple_items() {
    // Arrange
    let out_dir = ensure_output_dir();

    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss>
<channel>
<title>Multi Test</title>
<item><title>Ep 1</title><guid>e1</guid><enclosure url="http://x.com/1.mp3" length="1" type="audio/mpeg"/></item>
<item><title>Ep 2</title><guid>e2</guid><enclosure url="http://x.com/2.mp3" length="1" type="audio/mpeg"/></item>
<item><title>Ep 3</title><guid>e3</guid><enclosure url="http://x.com/3.mp3" length="1" type="audio/mpeg"/></item>
</channel>
</rss>"#;

    // Act
    let feed_id = 2033_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));

    // Assert: find all nfitems files for this feed_id
    let nfitems = output_records(&out_dir, "nfitems", feed_id);
    assert_eq!(nfitems.len(), 3);
}

// Edge case: itunes:image as text content (channel and item)
#[test]
fn test_itunes_image_text_content() {
    let out_dir = ensure_output_dir();

    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd">
<channel>
<title>Image Text</title>
<itunes:image>https://example.com/itunes-text.jpg</itunes:image>
<item>
<title>Episode</title>
<guid>ep</guid>
<enclosure url="https://example.com/ep.mp3" length="123" type="audio/mpeg"/>
<itunes:image>https://example.com/ep-text.jpg</itunes:image>
</item>
</channel>
</rss>"#;

    let feed_id = 2017_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));

    let mut nfitems = output_records(&out_dir, "nfitems", feed_id);
    assert_eq!(nfitems.len(), 1);
    let v = nfitems.pop().unwrap();

    assert_eq!(v["table"], "nfitems");
    assert_eq!(v["feed_id"], serde_json::json!(feed_id));
    assert_eq!(
        get_value(&v, "image"),
        Some(JsonValue::from("https://example.com/ep-text.jpg"))
    );

}


// fee="yes" should be treated as true in podcast:valueRecipient
#[test]
fn test_value_fee_yes() {
    let out_dir = ensure_output_dir();

    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss xmlns:podcast="https://podcastindex.org/namespace/1.0">
<channel>
<title>Fee Yes</title>
<item>
<title>Episode</title>
<guid>ep</guid>
<enclosure url="https://example.com/ep.mp3" length="123" type="audio/mpeg"/>
<podcast:value type="lightning" method="keysend">
    <podcast:valueRecipient name="App" type="node" address="addr" split="100" fee="yes"/>
</podcast:value>
</item>
</channel>
</rss>"#;

    let feed_id = 2019_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));

    let nfitems = output_records(&out_dir, "nfitems", feed_id);
    assert_eq!(nfitems.len(), 1);
    let item = &nfitems[0];

    let vb = get_value(item, "podcast_values").unwrap();
    assert_eq!(vb["destinations"][0]["fee"], true);
}

// fee="false" should be treated as false in podcast:valueRecipient
#[test]
fn test_value_fee_false() {
    let out_dir = ensure_output_dir();

    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss xmlns:podcast="https://podcastindex.org/namespace/1.0">
<channel>
<title>Fee False</title>
<item>
<title>Episode</title>
<guid>ep</guid>
<enclosure url="https://example.com/ep.mp3" length="123" type="audio/mpeg"/>
<podcast:value type="lightning" method="keysend">
    <podcast:valueRecipient name="App" type="node" address="addr" split="100" fee="false"/>
</podcast:value>
</item>
</channel>
</rss>"#;

    let feed_id = 20192_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));

    let nfitems = output_records(&out_dir, "nfitems", feed_id);
    assert_eq!(nfitems.len(), 1);
    let item = &nfitems[0];

    let vb = get_value(item, "podcast_values").unwrap();
    assert_eq!(vb["destinations"][0]["fee"], false);
}

// Missing fee attribute should default to false
#[test]
fn test_value_fee_missing() {
    let out_dir = ensure_output_dir();

    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss xmlns:podcast="https://podcastindex.org/namespace/1.0">
<channel>
<title>Fee Missing</title>
<item>
<title>Episode</title>
<guid>ep</guid>
<enclosure url="https://example.com/ep.mp3" length="123" type="audio/mpeg"/>
<podcast:value type="lightning" method="keysend">
    <podcast:valueRecipient name="App" type="node" address="addr" split="100"/>
</podcast:value>
</item>
</channel>
</rss>"#;

    let feed_id = 20193_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));

    let nfitems = output_records(&out_dir, "nfitems", feed_id);
    assert_eq!(nfitems.len(), 1);
    let item = &nfitems[0];

    let vb = get_value(item, "podcast_values").unwrap();
    assert_eq!(vb["destinations"][0]["fee"], false);
}


// Item value should fallback to first value when no lightning exists
#[test]
fn test_item_value_fallback_no_lightning() {
    let out_dir = ensure_output_dir();

    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss xmlns:podcast="https://podcastindex.org/namespace/1.0">
<channel>
<title>Item Fallback</title>
<item>
<title>Ep</title>
<guid>g-val</guid>
<enclosure url="https://example.com/ep.mp3" length="1" type="audio/mpeg"/>
<podcast:value type="bitcoin" method="keysend">
    <podcast:valueRecipient name="BTC" type="node" address="addr-btc" split="100"/>
</podcast:value>
<podcast:value type="HBD" method="keysend">
    <podcast:valueRecipient name="HBD" type="node" address="addr-hbd" split="100"/>
</podcast:value>
</item>
</channel>
</rss>"#;

    let feed_id = 20195_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));

    let nfitems = output_records(&out_dir, "nfitems", feed_id);
    assert_eq!(nfitems.len(), 1);
    let item = &nfitems[0];

    let vb = get_value(item, "podcast_values").unwrap();
    // Should fallback to first value (bitcoin) when no lightning
    assert_eq!(vb["model"]["type"], "bitcoin");
    assert_eq!(vb["destinations"][0]["name"], "BTC");
}

// Single non-lightning value should be used
#[test]
fn test_item_value_single_non_lightning() {
    let out_dir = ensure_output_dir();

    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss xmlns:podcast="https://podcastindex.org/namespace/1.0">
<channel>
<title>Single Value</title>
<item>
<title>Ep</title>
<guid>g-val</guid>
<enclosure url="https://example.com/ep.mp3" length="1" type="audio/mpeg"/>
<podcast:value type="bitcoin" method="keysend">
    <podcast:valueRecipient name="BTC" type="node" address="addr-btc" split="100"/>
</podcast:value>
</item>
</channel>
</rss>"#;

    let feed_id = 20196_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));

    let nfitems = output_records(&out_dir, "nfitems", feed_id);
    assert_eq!(nfitems.len(), 1);
    let item = &nfitems[0];

    let vb = get_value(item, "podcast_values").unwrap();
    // Should use the single value even though it's not lightning
    assert_eq!(vb["model"]["type"], "bitcoin");
    assert_eq!(vb["destinations"][0]["name"], "BTC");
}

// customKey and customValue should be preserved in recipients
#[test]
fn test_value_custom_attributes() {
    let out_dir = ensure_output_dir();

    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss xmlns:podcast="https://podcastindex.org/namespace/1.0">
<channel>
<title>Custom Attributes</title>
<item>
<title>Episode</title>
<guid>ep</guid>
<enclosure url="https://example.com/ep.mp3" length="123" type="audio/mpeg"/>
<podcast:value type="lightning" method="keysend">
    <podcast:valueRecipient name="App" type="node" address="addr" split="100" customKey="key1" customValue="value1"/>
</podcast:value>
</item>
</channel>
</rss>"#;

    let feed_id = 20197_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));

    let nfitems = output_records(&out_dir, "nfitems", feed_id);
    assert_eq!(nfitems.len(), 1);
    let item = &nfitems[0];

    let vb = get_value(item, "podcast_values").unwrap();
    let recipient = &vb["destinations"][0];
    // serde serializes Rust snake_case fields as-is, so it's "custom_key" and "custom_value"
    assert_eq!(recipient["custom_key"], "key1");
    assert_eq!(recipient["custom_value"], "value1");
}

// Multiple recipients with mixed fee values
#[test]
fn test_value_mixed_fees() {
    let out_dir = ensure_output_dir();

    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss xmlns:podcast="https://podcastindex.org/namespace/1.0">
<channel>
<title>Mixed Fees</title>
<item>
<title>Episode</title>
<guid>ep</guid>
<enclosure url="https://example.com/ep.mp3" length="123" type="audio/mpeg"/>
<podcast:value type="lightning" method="keysend">
    <podcast:valueRecipient name="Recipient1" type="node" address="addr1" split="50" fee="true"/>
    <podcast:valueRecipient name="Recipient2" type="node" address="addr2" split="50" fee="false"/>
    <podcast:valueRecipient name="Recipient3" type="node" address="addr3" split="0"/>
</podcast:value>
</item>
</channel>
</rss>"#;

    let feed_id = 20198_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));

    let nfitems = output_records(&out_dir, "nfitems", feed_id);
    assert_eq!(nfitems.len(), 1);
    let item = &nfitems[0];

    let vb = get_value(item, "podcast_values").unwrap();
    let destinations = vb["destinations"].as_array().unwrap();
    assert_eq!(destinations.len(), 3);
    assert_eq!(destinations[0]["fee"], true);
    assert_eq!(destinations[1]["fee"], false);
    assert_eq!(destinations[2]["fee"], false); // Missing fee defaults to false
}

// Items without a valid enclosure should be skipped entirely (including transcripts/value)
#[test]
fn test_skip_items_without_enclosure() {
    let out_dir = ensure_output_dir();

    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss xmlns:podcast="https://podcastindex.org/namespace/1.0">
<channel>
<title>No Enclosure</title>
<item>
<title>Missing Enclosure</title>
<guid>ep1</guid>
<podcast:transcript url="https://example.com/t1.json" type="application/json"/>
<podcast:value type="lightning" method="keysend">
    <podcast:valueRecipient name="App" type="node" address="addr" split="100"/>
</podcast:value>
</item>
</channel>
</rss>"#;

    let feed_id = 2020_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));

    let nfitems = output_records(&out_dir, "nfitems", feed_id);

    assert_eq!(nfitems.len(), 0);
}

// Atom <link rel="enclosure"> should be treated as enclosure
#[test]
fn test_atom_enclosure() {
    let out_dir = ensure_output_dir();

    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss xmlns:atom="http://www.w3.org/2005/Atom">
<channel>
<title>Atom Enclosure</title>
<item>
<title>Episode</title>
<guid>ep</guid>
<atom:link rel="enclosure" href="https://example.com/ep.mp3" length="555" type="audio/mpeg"/>
</item>
</channel>
</rss>"#;

    let feed_id = 2026_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));

    let mut nfitems = output_records(&out_dir, "nfitems", feed_id);
    assert_eq!(nfitems.len(), 1);
    let item = nfitems.pop().unwrap();

    assert_eq!(get_value(&item, "enclosure_url"), Some(JsonValue::from("https://example.com/ep.mp3")));
    assert_eq!(get_value(&item, "enclosure_length"), Some(JsonValue::from(555)));
    assert_eq!(get_value(&item, "enclosure_type"), Some(JsonValue::from("audio/mpeg")));
}

// itunes:explicit boolean true/false should be honored for channel and item
#[test]
fn test_itunes_explicit_boolean() {
    let out_dir = ensure_output_dir();

    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd">
<channel>
<title>Explicit Bool</title>
<itunes:explicit>true</itunes:explicit>
<item>
<title>Episode</title>
<guid>ep</guid>
<itunes:explicit>false</itunes:explicit>
<enclosure url="https://example.com/ep.mp3" length="1" type="audio/mpeg"/>
</item>
</channel>
</rss>"#;

    let feed_id = 2027_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));

    let nf = single_record(&out_dir, "newsfeeds", feed_id);
    let nfitems = output_records(&out_dir, "nfitems", feed_id);

    assert_eq!(get_value(&nf, "explicit"), Some(JsonValue::from(1)));

    assert_eq!(nfitems.len(), 1);
    let item = &nfitems[0];

    assert_eq!(get_value(item, "itunes_explicit"), Some(JsonValue::from(0)));
}

// Soundbite and person fields should be truncated to Partytime limits
#[test]
fn test_soundbite_and_person_truncation() {
    let out_dir = ensure_output_dir();

    let long_title = "x".repeat(600);
    let long_name = "n".repeat(200);
    let long_role = "R".repeat(200);
    let long_group = "G".repeat(200);
    let long_img = format!("https://example.com/{}.jpg", "i".repeat(800));
    let long_href = format!("https://example.com/{}", "h".repeat(800));

    let feed = format!(r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss xmlns:podcast="https://podcastindex.org/namespace/1.0">
<channel>
<title>Truncate</title>
<item>
<title>Episode</title>
<guid>ep</guid>
<enclosure url="https://example.com/ep.mp3" length="1" type="audio/mpeg"/>
<podcast:soundbite startTime="0" duration="1">{long_title}</podcast:soundbite>
<podcast:person role="{long_role}" group="{long_group}" img="{long_img}" href="{long_href}">{long_name}</podcast:person>
</item>
</channel>
</rss>"#);

    process_feed_sync(Cursor::new(feed), "test.xml", Some(2028));

    let nfitems = output_records(&out_dir, "nfitems", 2028);
    assert_eq!(nfitems.len(), 1);
    let item = &nfitems[0];

    let soundbites_val = get_value(item, "podcast_soundbites").unwrap();
    let soundbites_array = soundbites_val.as_array().unwrap();
    assert_eq!(soundbites_array.len(), 1);
    let sb_title = soundbites_array[0].get("title")
        .unwrap()
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(sb_title.len(), 500);

    let persons_val = get_value(item, "podcast_persons").unwrap();
    let persons_array = persons_val.as_array().unwrap();
    assert_eq!(persons_array.len(), 1);
    let name = persons_array[0].get("name")
        .unwrap()
        .as_str()
        .unwrap()
        .to_string();
    let role = persons_array[0].get("role")
        .unwrap()
        .as_str()
        .unwrap()
        .to_string();
    let group = persons_array[0].get("group")
        .unwrap()
        .as_str()
        .unwrap()
        .to_string();
    let img = persons_array[0].get("img")
        .unwrap()
        .as_str()
        .unwrap()
        .to_string();
    let href = persons_array[0].get("href")
        .unwrap()
        .as_str()
        .unwrap()
        .to_string();

    assert_eq!(name.len(), 128);
    assert_eq!(role.len(), 128);
    assert_eq!(group.len(), 128);
    assert_eq!(img.len(), 768);
    assert_eq!(href.len(), 768);
}

// First enclosure wins and type is guessed when missing
#[test]
fn test_enclosure_first_and_type_guess() {
    let out_dir = ensure_output_dir();

    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss>
<channel>
<title>Enclosures</title>
<item>
<title>Episode</title>
<guid>ep</guid>
<enclosure url="https://example.com/first.mp3" length="123"/>
<enclosure url="https://example.com/second.ogg" length="999" type="audio/ogg"/>
</item>
</channel>
</rss>"#;

    let feed_id = 2022_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));

    let mut nfitems = output_records(&out_dir, "nfitems", feed_id);
    assert_eq!(nfitems.len(), 1);
    let item = nfitems.pop().unwrap();

    assert_eq!(get_value(&item, "enclosure_url"), Some(JsonValue::from("https://example.com/first.mp3")));
    assert_eq!(get_value(&item, "enclosure_type"), Some(JsonValue::from("audio/mpeg")));
    assert_eq!(get_value(&item, "enclosure_length"), Some(JsonValue::from(123)));
}

// Truncation/clamp behavior should mirror partytime.js limits
#[test]
fn test_partytime_truncation_and_clamps() {
    let out_dir = ensure_output_dir();

    let long_title = "T".repeat(1500);
    let long_guid = "G".repeat(900);
    let long_type = "audio/verylongtype".repeat(20);
    let long_owner = "owner@example.com-".repeat(20);

    let feed = format!(
        r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd"
 xmlns:podcast="https://podcastindex.org/namespace/1.0">
<channel>
<title>Clamp Feed</title>
<language>abcdefghijklmnop</language>
<podcast:locked owner="{owner}">true</podcast:locked>
<item>
<title>{title}</title>
<guid>{guid}</guid>
<enclosure url="https://example.com/audio.mp3" length="9999999999999999999" type="{etype}"/>
<itunes:episode>9999999999</itunes:episode>
</item>
</channel>
</rss>"#,
        owner = long_owner,
        title = long_title,
        guid = long_guid,
        etype = long_type
    );

    let feed_id = 30305_i64;

    process_feed_sync(Cursor::new(feed), "clamp.xml", Some(feed_id));

    let item = single_record(&out_dir, "nfitems", feed_id);
    let news = single_record(&out_dir, "newsfeeds", feed_id);

    let title = get_value(&item, "title")
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap();
    assert_eq!(title.len(), 1024);
    let guid = get_value(&item, "guid")
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap();
    assert_eq!(guid.len(), 740);
    let enclosure_type = get_value(&item, "enclosure_type")
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap();
    assert_eq!(enclosure_type.len(), 128);
    assert_eq!(get_value(&item, "enclosure_length"), Some(JsonValue::from(0)));
    let episode_val = get_value(&item, "itunes_episode");
    // itunes_episode should be clamped to 1_000_000 max
    assert_eq!(episode_val, Some(JsonValue::from(1_000_000)));

    assert_eq!(
        get_value(&news, "language"),
        Some(JsonValue::from("abcdefgh"))
    );
    let owner_val = get_value(&news, "podcast_owner")
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap();
    assert_eq!(owner_val.len(), 255);
}

// itunes:duration should normalize to seconds (including mm:ss format)
#[test]
fn test_duration_normalization() {
    let out_dir = ensure_output_dir();

    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd">
<channel>
<title>Duration</title>
<item>
<title>Episode</title>
<guid>ep</guid>
<itunes:duration>01:02</itunes:duration>
<enclosure url="https://example.com/ep.mp3" length="1" type="audio/mpeg"/>
</item>
</channel>
</rss>"#;

    process_feed_sync(Cursor::new(feed), "test.xml", Some(2023));

    let nfitems = output_records(&out_dir, "nfitems", 2023);
    assert_eq!(nfitems.len(), 1);
    assert_eq!(get_value(&nfitems[0], "itunes_duration"), Some(JsonValue::from(62)));
}

// Value type should be mapped to numeric codes (HBD=1, bitcoin=2, lightning=0)
#[test]
fn test_value_type_mapping() {
    let out_dir = ensure_output_dir();

    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss xmlns:podcast="https://podcastindex.org/namespace/1.0">
<channel>
<title>Value Types</title>
<podcast:value type="bitcoin" method="custom">
<podcast:valueRecipient name="BTC" type="node" address="addr" split="100"/>
</podcast:value>
<item>
<title>Episode</title>
<guid>ep</guid>
<enclosure url="https://example.com/ep.mp3" length="1" type="audio/mpeg"/>
<podcast:value type="HBD" method="keysend">
    <podcast:valueRecipient name="App" type="node" address="addr" split="100"/>
</podcast:value>
</item>
</channel>
</rss>"#;

    let feed_id = 2024_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));

    let newsfeed = single_record(&out_dir, "newsfeeds", feed_id);
    let item = single_record(&out_dir, "nfitems", feed_id);

    // Check that channel value is in newsfeeds (not nfvalue table)
    let channel_value = get_value(&newsfeed, "podcast_value").unwrap();
    assert_eq!(channel_value["model"]["type"], "bitcoin");

    // Check that item value is in nfitems (not nfitem_value table)
    let item_value = get_value(&item, "podcast_values").unwrap();
    assert_eq!(item_value["model"]["type"], "HBD");
}


// Edge case: When guid is missing, enclosure URL should be used as the GUID
#[test]
fn test_guid_fallback_to_enclosure() {
    let out_dir = ensure_output_dir();

    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss>
<channel>
<title>GuidFallback</title>
<item>
<title>Episode</title>
<enclosure url="https://example.com/ep.mp3" length="1" type="audio/mpeg"/>
</item>
</channel>
</rss>"#;

    let feed_id = 30304_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));

    let mut nfitems = output_records(&out_dir, "nfitems", feed_id);
    assert_eq!(nfitems.len(), 1);
    let item = nfitems.pop().unwrap();

    assert_eq!(
        get_value(&item, "guid"),
        Some(JsonValue::from("https://example.com/ep.mp3"))
    );
}


// itunes:episodeType should be emitted for items
#[test]
fn test_itunes_episode_type_output() {
    let out_dir = ensure_output_dir();

    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd">
<channel>
<title>EpisodeType</title>
<item>
<title>Ep</title>
<guid>g-type</guid>
<itunes:episodeType>bonus</itunes:episodeType>
<enclosure url="https://example.com/ep.mp3" length="1" type="audio/mpeg"/>
</item>
</channel>
</rss>"#;

    let feed_id = 33003_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));

    let mut nfitems = output_records(&out_dir, "nfitems", feed_id);
    assert_eq!(nfitems.len(), 1);
    let item = nfitems.pop().unwrap();

    assert_eq!(
        get_value(&item, "itunes_episode_type"),
        Some(JsonValue::from("bonus"))
    );
}

// Item-level podcast:value should prefer lightning when multiple blocks exist
#[test]
fn test_item_value_lightning_priority() {
    let out_dir = ensure_output_dir();

    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss xmlns:podcast="https://podcastindex.org/namespace/1.0">
<channel>
<title>Item Value Priority</title>
<item>
<title>Ep</title>
<guid>g-val</guid>
<enclosure url="https://example.com/ep.mp3" length="1" type="audio/mpeg"/>
<podcast:value type="bitcoin" method="keysend">
    <podcast:valueRecipient name="BTC" type="node" address="addr-btc" split="100"/>
</podcast:value>
<podcast:value type="lightning" method="keysend">
    <podcast:valueRecipient name="LN" type="node" address="addr-ln" split="100"/>
</podcast:value>
</item>
</channel>
</rss>"#;

    let feed_id = 33004_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));

    let nfitems = output_records(&out_dir, "nfitems", feed_id);
    assert_eq!(nfitems.len(), 1);
    let item = &nfitems[0];

    let vb = get_value(item, "podcast_values").unwrap();
    // Lightning should be selected (priority over bitcoin)
    assert_eq!(vb["model"]["type"], "lightning");
    assert_eq!(vb["destinations"][0]["name"], "LN");
}

// content:encoded should be used as a fallback when description/summary are missing
#[test]
fn test_content_encoded_fallback_description() {
    let out_dir = ensure_output_dir();

    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0"?>
<rss xmlns:content="http://purl.org/rss/1.0/modules/content/">
<channel>
<title>Content Encoded</title>
<item>
<title>Ep</title>
<guid>g-content</guid>
<content:encoded><![CDATA[<p>Rich description</p>]]></content:encoded>
<enclosure url="https://example.com/ep.mp3" length="1" type="audio/mpeg"/>
</item>
</channel>
</rss>"#;

    process_feed_sync(Cursor::new(feed), "test.xml", Some(33005));

    let mut items = output_records(&out_dir, "nfitems", 33005);
    assert_eq!(items.len(), 1);
    let item = items.pop().unwrap();

    assert_eq!(
        get_value(&item, "description"),
        Some(JsonValue::from("<p>Rich description</p>"))
    );
}


// Atom feeds should mirror Partytime behavior (alternate links, enclosures, pubsub)
#[test]
fn test_atom_feed_support() {
    let out_dir = ensure_output_dir();

    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/atom.xml
1700000001
<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>Atom Cast</title>
  <subtitle>Atom Description</subtitle>
  <link rel="alternate" href="https://example.com/atom"/>
  <link rel="hub" href="https://pubsubhubbub.appspot.com/"/>
  <link rel="self" href="https://example.com/atom.xml"/>
  <logo>https://example.com/logo.png</logo>
  <entry>
<id>tag:example.com,2024:1</id>
<title>Atom Episode</title>
<updated>2024-01-01T00:00:00Z</updated>
<summary>Atom entry summary.</summary>
<link rel="alternate" href="https://example.com/atom/1"/>
<link rel="enclosure" href="https://example.com/audio.mp3" length="1234" type="audio/mpeg"/>
  </entry>
</feed>"#;

    let feed_id = 55001_i64;
    process_feed_sync(Cursor::new(feed), "atom.xml", Some(feed_id));

    let nf = single_record(&out_dir, "newsfeeds", feed_id);
    let nfitems = output_records(&out_dir, "nfitems", feed_id);

    assert_eq!(get_value(&nf, "link"), Some(json!("https://example.com/atom")));
    assert_eq!(get_value(&nf, "description"), Some(json!("Atom Description")));
    assert_eq!(get_value(&nf, "image"), Some(json!("https://example.com/logo.png")));
    assert_eq!(get_value(&nf, "pubsub_hub_url"), Some(json!("https://pubsubhubbub.appspot.com/")));
    assert_eq!(get_value(&nf, "pubsub_self_url"), Some(json!("https://example.com/atom.xml")));

    assert_eq!(nfitems.len(), 1);
    let item = &nfitems[0];

    assert_eq!(get_value(item, "link"), Some(json!("https://example.com/atom/1")));
    assert_eq!(get_value(item, "pub_date"), Some(json!(1704067200)));
    assert_eq!(get_value(item, "enclosure_url"), Some(json!("https://example.com/audio.mp3")));
    assert_eq!(get_value(item, "enclosure_length"), Some(json!(1234)));
    assert_eq!(get_value(item, "enclosure_type"), Some(json!("audio/mpeg")));
    assert_eq!(get_value(item, "description"), Some(json!("Atom entry summary.")));
}

#[test]
fn test_atom_feed_author() {
    let out_dir = ensure_output_dir();

    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/atom.xml
1700000001
<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>Atom Feed with Author</title>
  <subtitle>Description</subtitle>
  <author>
    <name>Atom Author Name</name>
    <email>author@example.com</email>
  </author>
  <entry>
    <id>tag:example.com,2024:1</id>
    <title>Episode</title>
    <updated>2024-01-01T00:00:00Z</updated>
    <link rel="enclosure" href="https://example.com/ep.mp3" length="123" type="audio/mpeg"/>
  </entry>
</feed>"#;

    let feed_id = 55002_i64;
    process_feed_sync(Cursor::new(feed), "atom.xml", Some(feed_id));

    let nf = single_record(&out_dir, "newsfeeds", feed_id);

    // Atom author fields should be stored separately
    assert_eq!(get_value(&nf, "atom_author_name"), Some(json!("Atom Author Name")));
    assert_eq!(get_value(&nf, "atom_author_email"), Some(json!("author@example.com")));
}

#[test]
fn test_preserve_spaces_in_itunes_title() {
    let out_dir = ensure_output_dir();

    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<rss xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd"
 xmlns:podcast="https://podcastindex.org/namespace/1.0">
<channel>
<title>Preserve leading/trailing spaces iTunes Item Titles</title>
<item>
<title><![CDATA[Ep ]]></title>
<itunes:title><![CDATA[Ep ]]></itunes:title>
<guid>g-content</guid>
<enclosure url="https://example.com/ep.mp3" length="1" type="audio/mpeg"/>
</item>
<item>
<title><![CDATA[Ep2 ]]></title>
<guid>g-content</guid>
<enclosure url="https://example.com/ep.mp3" length="1" type="audio/mpeg"/>
</item>
</channel>
</rss>"#;

    let feed_id = 33006_i64;
    process_feed_sync(Cursor::new(feed), "test.xml", Some(feed_id));

    let mut nfitems = output_records(&out_dir, "nfitems", feed_id);
    assert_eq!(nfitems.len(), 2);
    let item1 = nfitems.remove(0);
    let item2 = nfitems.remove(0);

    assert_eq!(
        get_value(&item2, "title"),
        Some(JsonValue::from("Ep2"))
    );

    assert_eq!(
        get_value(&item1, "title"),
        Some(JsonValue::from("Ep "))
    );
}

#[test]
fn test_description_precedence() {
    let out_dir = ensure_output_dir();

    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0"
     xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd"
     xmlns:content="http://purl.org/rss/1.0/modules/content/">

  <channel>
    <title>Example Feed</title>

    <!-- Case 1: iTunes summary (common in podcast feeds) -->
    <item>
      <title>Episode with iTunes Summary</title>
      <itunes:summary>This is the iTunes summary description for the podcast episode.</itunes:summary>
      <guid>case-1</guid>
      <enclosure url="https://example.com/case1.mp3" length="1" type="audio/mpeg"/>
    </item>

    <!-- Case 2: content:encoded within description (WordPress/blog feeds) -->
    <item>
      <title>Blog Post with Content Encoded</title>
      <description></description>
      <content:encoded><![CDATA[
        <p>This is the full HTML content of the blog post with formatting.</p>
        <p>It can contain multiple paragraphs and rich content.</p>
      ]]></content:encoded>
      <enclosure url="https://example.com/ep.mp3" length="1" type="audio/mpeg"/>
   </item>

    <!-- Case 3: Simple description field -->
    <item>
      <title>Simple Description</title>
      <description>This is a plain text description of the item.</description>
      <enclosure url="https://example.com/ep.mp3" length="1" type="audio/mpeg"/>
    </item>

    <!-- Case 4: content field as array (parsed from Atom feeds) -->
    <item>
      <title>Atom-style Content</title>
      <content type="html">
        <![CDATA[<p>First content element</p>]]>
      </content>
      <content type="text">
        Second content element (would be ignored)
      </content>
      <enclosure url="https://example.com/ep.mp3" length="1" type="audio/mpeg"/>
    </item>

    <!-- Case 5: content with #text property (from XML parsing) -->
    <item>
      <title>Content with Text Node</title>
      <content type="html">
        This text becomes the #text property when parsed
      </content>
      <enclosure url="https://example.com/ep.mp3" length="1" type="audio/mpeg"/>
    </item>

    <!-- Case 6: Multiple sources - iTunes takes priority -->
    <item>
      <title>Multiple Description Sources</title>
      <itunes:summary>iTunes summary (wins)</itunes:summary>
      <description>Regular description (ignored)</description>
      <content:encoded>Content encoded</content:encoded>
      <enclosure url="https://example.com/ep.mp3" length="1" type="audio/mpeg"/>
    </item>

    <!-- Case 7: No description at all -->
    <item>
      <title>No Description</title>
      <!-- Will result in empty string -->
      <enclosure url="https://example.com/ep.mp3" length="1" type="audio/mpeg"/>
    </item>

  </channel>
</rss>"#;

    process_feed_sync(Cursor::new(feed), "test.xml", Some(33007));

    let nfitems_files = output_records(&out_dir, "nfitems", 33007);
    assert_eq!(nfitems_files.len(), 7);

    assert_eq!(get_value(&nfitems_files[0], "description"),Some(JsonValue::from(
        "This is the iTunes summary description for the podcast episode."
    )));

    assert_eq!(get_value(&nfitems_files[1], "description"), Some(JsonValue::from(
        "<p>This is the full HTML content of the blog post with formatting.</p>\n        <p>It can contain multiple paragraphs and rich content.</p>"
    )));

    assert_eq!(get_value(&nfitems_files[2], "description"), Some(JsonValue::from("This is a plain text description of the item.")));
    assert_eq!(get_value(&nfitems_files[3], "description"), Some(JsonValue::from("<p>First content element</p>")));
    assert_eq!(get_value(&nfitems_files[4], "description"), Some(JsonValue::from("This text becomes the #text property when parsed")));
    assert_eq!(get_value(&nfitems_files[5], "description"), Some(JsonValue::from("Content encoded")));
    assert_eq!(get_value(&nfitems_files[6], "description"), Some(JsonValue::from("")));
}

#[test]
fn test_alternate_enclosures() {
    let out_dir = ensure_output_dir();

    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0"
    xmlns:podcast="https://podcastindex.org/namespace/1.0"
    xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd"
    xmlns:content="http://purl.org/rss/1.0/modules/content/">

  <channel>
    <title>Example Feed</title>
    <item>
      <itunes:episodeType>bonus</itunes:episodeType>
      <enclosure url="https://feeds.fountain.fm/40huHEEF6JMPGYctMuUI/items/WxMQ7HpjU1XJpgUbo1Fm/files/AUDIO---DEFAULT---a757e3df-28f7-4b38-a704-e0e7780c70f9.mp3" length="2397614" type="audio/mpeg"/>
      <itunes:duration>83</itunes:duration>
      <podcast:chapters url="https://feeds.fountain.fm/40huHEEF6JMPGYctMuUI/items/WxMQ7HpjU1XJpgUbo1Fm/files/AUDIO---CHAPTERS---DEFAULT---PODCAST.json" type="application/json+chapters"/>
      <podcast:transcript url="https://feeds.fountain.fm/40huHEEF6JMPGYctMuUI/items/WxMQ7HpjU1XJpgUbo1Fm/files/AUDIO---TRANSCRIPT---DEFAULT---SRT.srt" type="application/x-subrip" rel="captions"/>
      <podcast:alternateEnclosure type="audio/mpeg" length="78858122" title="Bonus Episode" paywall="L402" auth="NOSTR">
        <itunes:duration>3269</itunes:duration>
        <podcast:chapters url="https://feeds.fountain.fm/40huHEEF6JMPGYctMuUI/items/WxMQ7HpjU1XJpgUbo1Fm/files/AUDIO---CHAPTERS---PAID---PODCAST.json" type="application/json+chapters"/>
        <podcast:transcript url="https://feeds.fountain.fm/40huHEEF6JMPGYctMuUI/items/WxMQ7HpjU1XJpgUbo1Fm/files/AUDIO---TRANSCRIPT---PAID---SRT.srt" type="application/x-subrip" rel="captions"/>
        <podcast:source uri="https://feeds.fountain.fm/40huHEEF6JMPGYctMuUI/items/WxMQ7HpjU1XJpgUbo1Fm/files/AUDIO---PAID---91408357-3379-407d-a8b3-85b3cc2d3349.mp3"/>
      </podcast:alternateEnclosure>
      <guid isPermaLink="false">0bf8aeaf-9f2b-4008-812d-e9389e4639f7</guid>
      <pubDate>Thu, 24 Jul 2025 19:35:57 GMT</pubDate>
      <title>Bonus 01: Living in the Shadow of Bitcoin</title>
      <description>&lt;p&gt;A viral anti-Bitcoin video spreads fear through emotional storytelling and slick sound design. But what's the real story behind the drama, and why does it matter?&lt;/p&gt;</description>
      <itunes:explicit>false</itunes:explicit>
      <itunes:image href="https://feeds.fountain.fm/40huHEEF6JMPGYctMuUI/items/WxMQ7HpjU1XJpgUbo1Fm/files/CHAPTER_ART---DEFAULT---e32b13c4-9bdf-4102-9ff0-4c4fc72462a9.jpg"/>
    </item>
  </channel>
</rss>"#;

    process_feed_sync(Cursor::new(feed), "test.xml", Some(33008));

    let nfitems_files = output_records(&out_dir, "nfitems", 33008);
    assert_eq!(nfitems_files.len(), 1);

    assert_eq!(get_value(&nfitems_files[0], "itunes_duration"), Some(JsonValue::from(83)));
    // assert_eq!(get_value(&nfitems_files[1], "duration"), Some(JsonValue::from(3269)));
}

#[test]
fn test_ignore_duplicate_channel_tags() {
    let out_dir = ensure_output_dir();

    let feed = r#"1700000000
[[NO_ETAG]]
https://example.com/feed.xml
1700000001
<rss xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd"
 xmlns:podcast="https://podcastindex.org/namespace/1.0">
<channel>
<title>Channel Title</title>
<generator>Channel Generator</generator>
<generator>Another Channel Generator</generator>
<link>http://example.com/link-channel</link>
<link>http://example.com/another-link-channel</link>
<description>Channel Description</description>
<description>Another Channel Description</description>
<itunes:author>Itunes Channel Author</itunes:author>
<itunes:author>Another Itunes Channel Author</itunes:author>
<itunes:new-feed-url>http://example.com/new-feed-url</itunes:new-feed-url>
<itunes:new-feed-url>http://example.com/another-new-feed-url</itunes:new-feed-url>
</channel>
</rss>"#;

    process_feed_sync(Cursor::new(feed), "test.xml", Some(33009));

    let nf = single_record(&out_dir, "newsfeeds", 33009);
    println!("{:?}", nf);
    assert_eq!(get_value(&nf, "generator"), Some(JsonValue::from("Channel Generator")));
    assert_eq!(get_value(&nf, "link"), Some(JsonValue::from("http://example.com/link-channel")));
    assert_eq!(get_value(&nf, "description"), Some(JsonValue::from("Channel Description")));
    assert_eq!(get_value(&nf, "itunes_author"), Some(JsonValue::from("Itunes Channel Author")));
    assert_eq!(get_value(&nf, "itunes_new_feed_url"), Some(JsonValue::from("http://example.com/new-feed-url")));
}

#[test]
fn test_nfitems_item_itunes_author() {
    let out_dir = ensure_output_dir();
    let feed = r#"0
[[NO_ETAG]]
https://example.com/feed.xml
0
<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd">
  <channel>
    <title>Channel</title>
    <item>
      <title>Episode A</title>
      <itunes:author> Guest Speaker </itunes:author>
      <enclosure url="https://example.com/ep.mp3" length="123" type="audio/mpeg"/>
    </item>
  </channel>
</rss>"#;
    let feed_id = 808002_i64;
    process_feed_sync(Cursor::new(feed), "<test>", Some(feed_id));
    let v = single_record(&out_dir, "nfitems", feed_id);
    assert_eq!(v["table"], "nfitems");
    // itunes_author is not trimmed in current implementation
    assert_eq!(get_value(&v, "itunes_author"), Some(serde_json::json!(" Guest Speaker ")));
}