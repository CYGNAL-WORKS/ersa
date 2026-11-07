//! A user message's attachments as Claude Code takes them (PROTO-CLAUDE-CODE-STREAM,
//! User message): after the user's text block, one base64 `image` block per
//! image, then one text block listing the files and folders, each after
//! `file: ` or `folder: `, between an `<ersa-attachments>` line and a
//! `</ersa-attachments>` line.
//!
//! Sending builds the blocks; projecting and listing read them back, so an
//! image or the list never reaches a message's text.

use ersa_adapter::{OutgoingAttachment, OutgoingMessage};
use ersa_protocol::events::MessageAttachment;
use serde_json::Value;

use crate::wire::{ContentBlock, ImageSource, InputBlock};

const OPEN: &str = "<ersa-attachments>";
const CLOSE: &str = "</ersa-attachments>";

/// The content blocks of a user message with `message`'s text and
/// attachments, in the order the CLI takes them (PROTO-CLAUDE-CODE-STREAM-R25).
pub(crate) fn blocks(message: &OutgoingMessage) -> Vec<InputBlock> {
    let mut blocks = Vec::new();
    if !message.text.is_empty() {
        blocks.push(InputBlock::Text {
            text: message.text.clone(),
        });
    }
    let mut listed = Vec::new();
    for attachment in &message.attachments {
        match attachment {
            OutgoingAttachment::Image { media_type, data } => blocks.push(InputBlock::Image {
                source: ImageSource {
                    kind: "base64".into(),
                    media_type: media_type.clone(),
                    data: data.clone(),
                },
            }),
            OutgoingAttachment::File { path } => listed.push(format!("file: {path}")),
            OutgoingAttachment::Folder { path } => listed.push(format!("folder: {path}")),
        }
    }
    if !listed.is_empty() {
        blocks.push(InputBlock::Text {
            text: format!(
                "{OPEN}
{}
{CLOSE}",
                listed.join(
                    "
"
                )
            ),
        });
    }
    blocks
}

/// Splits a user message's content blocks into its text, the text blocks
/// joined by line breaks, and its attachments, in the order they came
/// (PROTO-CLAUDE-CODE-STREAM-R26, R27). The text is `None` when the message
/// holds no text of its own.
pub(crate) fn split(blocks: &[Value]) -> (Option<String>, Vec<MessageAttachment>) {
    let mut texts = Vec::new();
    let mut images = Vec::new();
    let mut listed = Vec::new();
    for block in blocks {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                let Some(text) = block.get("text").and_then(Value::as_str) else {
                    continue;
                };
                match list(text) {
                    Some(items) => listed.extend(items),
                    None => texts.push(text),
                }
            }
            Some("image") => {
                let source = &block["source"];
                if let (Some(media_type), Some(data)) = (
                    source.get("media_type").and_then(Value::as_str),
                    source.get("data").and_then(Value::as_str),
                ) {
                    images.push(MessageAttachment::Image {
                        media_type: media_type.into(),
                        data: data.into(),
                    });
                }
            }
            _ => {}
        }
    }
    // Images come before the list on the wire, so before the files and folders here.
    images.extend(listed);
    let text = (!texts.is_empty()).then(|| texts.join("\n"));
    (text, images)
}

/// [`split`] for blocks as the session record holds them.
pub(crate) fn split_blocks(blocks: &[ContentBlock]) -> (Option<String>, Vec<MessageAttachment>) {
    let values: Vec<Value> = blocks
        .iter()
        .filter_map(|block| serde_json::to_value(block).ok())
        .collect();
    split(&values)
}

/// The files and folders of a text block that is exactly an attachments list,
/// else `None`: a list the user typed among other words stays their text.
fn list(text: &str) -> Option<Vec<MessageAttachment>> {
    let inner = text.strip_prefix(OPEN)?.strip_suffix(CLOSE)?;
    let inner = inner.strip_prefix('\n')?.strip_suffix('\n')?;
    inner
        .split('\n')
        .map(|line| {
            if let Some(path) = line.strip_prefix("file: ") {
                Some(MessageAttachment::File { path: path.into() })
            } else {
                line.strip_prefix("folder: ")
                    .map(|path| MessageAttachment::Folder { path: path.into() })
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The blocks as they go on the wire.
    fn wire(message: &OutgoingMessage) -> Vec<Value> {
        blocks(message)
            .iter()
            .map(|b| serde_json::to_value(b).unwrap())
            .collect()
    }

    fn message(text: &str, attachments: Vec<OutgoingAttachment>) -> OutgoingMessage {
        OutgoingMessage {
            text: text.into(),
            attachments,
        }
    }

    // verifies: PROTO-CLAUDE-CODE-STREAM-R25
    #[test]
    fn the_text_then_each_image_then_one_list_of_files_and_folders() {
        let sent = wire(&message(
            "Look at these",
            vec![
                OutgoingAttachment::File {
                    path: "notes/todo.md".into(),
                },
                OutgoingAttachment::Image {
                    media_type: "image/png".into(),
                    data: "AAAA".into(),
                },
                OutgoingAttachment::Folder { path: "src".into() },
                OutgoingAttachment::Image {
                    media_type: "image/gif".into(),
                    data: "BBBB".into(),
                },
            ],
        ));
        assert_eq!(
            sent,
            vec![
                json!({ "type": "text", "text": "Look at these" }),
                json!({ "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "AAAA" } }),
                json!({ "type": "image", "source": { "type": "base64", "media_type": "image/gif", "data": "BBBB" } }),
                json!({ "type": "text", "text": "<ersa-attachments>\nfile: notes/todo.md\nfolder: src\n</ersa-attachments>" }),
            ]
        );
    }

    // verifies: PROTO-CLAUDE-CODE-STREAM-R25
    #[test]
    fn no_text_block_for_a_message_of_attachments_alone_and_no_list_without_paths() {
        let sent = wire(&message(
            "",
            vec![OutgoingAttachment::Image {
                media_type: "image/png".into(),
                data: "AAAA".into(),
            }],
        ));
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0]["type"], "image");
    }

    // verifies: PROTO-CLAUDE-CODE-STREAM-R26
    #[test]
    fn reading_back_gives_the_text_and_the_attachments_in_order() {
        let sent = wire(&message(
            "Look at these",
            vec![
                OutgoingAttachment::Image {
                    media_type: "image/png".into(),
                    data: "AAAA".into(),
                },
                OutgoingAttachment::File {
                    path: "notes/todo.md".into(),
                },
                OutgoingAttachment::Folder { path: "src".into() },
            ],
        ));
        let (text, attachments) = split(&sent);
        assert_eq!(text.as_deref(), Some("Look at these"));
        assert_eq!(
            attachments,
            vec![
                MessageAttachment::Image {
                    media_type: "image/png".into(),
                    data: "AAAA".into()
                },
                MessageAttachment::File {
                    path: "notes/todo.md".into()
                },
                MessageAttachment::Folder { path: "src".into() },
            ]
        );
    }

    // verifies: PROTO-CLAUDE-CODE-STREAM-R26
    #[test]
    fn a_list_among_other_words_or_with_a_stray_line_is_the_users_text() {
        for typed in [
            "See <ersa-attachments>\nfile: a\n</ersa-attachments>",
            "<ersa-attachments>\nfile: a\nnot a path line\n</ersa-attachments>",
            "<ersa-attachments>\n</ersa-attachments>",
        ] {
            let (text, attachments) = split(&[json!({ "type": "text", "text": typed })]);
            assert_eq!(text.as_deref(), Some(typed));
            assert!(attachments.is_empty());
        }
    }
}
