//! Claude Code's questions (PROTO-CLAUDE-CODE-STREAM, Questions): the
//! `AskUserQuestion` tool's input as `question.asked`, an answer in Ersa's
//! words as the `answers` object the CLI takes, and that object read back as
//! `question.answered`'s answers.
//!
//! The CLI keys each answer by its question's text and gives it as a string:
//! the chosen option's label, the chosen labels joined by ", " for a
//! multi-select question, or the user's own words. Ersa names questions and
//! options by their index instead, so this module translates both ways.

use ersa_protocol::events::{Answer, AnswerValue, Question, QuestionOption};
use serde_json::{Map, Value};

/// The tool through which Claude Code asks the user a question. A
/// `can_use_tool` request for it is a question, not a permission request.
pub const QUESTION_TOOL: &str = "AskUserQuestion";

/// What the CLI puts between the labels of a multi-select answer.
const LABEL_SEPARATOR: &str = ", ";

/// The questions of an `AskUserQuestion` input (`{ questions: [{ question,
/// header, options: [{ label, description }], multiSelect }] }`), each named by
/// its index, its options by theirs. Every Claude Code question takes the
/// user's own words beside its options. A question item without text is
/// still listed, with empty text, so the indices stay those of the input;
/// the second value says how many items were malformed that way.
pub fn asked(input: &Map<String, Value>) -> (Vec<Question>, usize) {
    let items = input
        .get("questions")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let mut malformed = 0;
    let questions = items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let text = |key: &str| item.get(key).and_then(Value::as_str).map(String::from);
            let question = text("question").unwrap_or_else(|| {
                malformed += 1;
                String::new()
            });
            let options = item
                .get("options")
                .and_then(Value::as_array)
                .map(|options| {
                    options
                        .iter()
                        .enumerate()
                        .map(|(index, option)| {
                            let text = |key: &str| {
                                option.get(key).and_then(Value::as_str).map(String::from)
                            };
                            QuestionOption {
                                id: index.to_string(),
                                label: text("label").unwrap_or_default(),
                                description: text("description"),
                            }
                        })
                        .collect()
                });
            Question {
                id: index.to_string(),
                text: question,
                header: text("header"),
                options,
                multi_select: item
                    .get("multiSelect")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                takes_text: true,
            }
        })
        .collect();
    (questions, malformed)
}

/// The `answers` object the CLI takes for `answers` to `questions`: one entry
/// per question text. Refused, with the reason, unless every question has
/// exactly one answer, each option id is one the question offers, and a list
/// of ids answers only a multi-select question. Questions sharing one text
/// share one key in the CLI's answer, so they must be given the same answer.
pub fn to_cli(questions: &[Question], answers: &[Answer]) -> Result<Map<String, Value>, String> {
    let mut given: Vec<Option<String>> = vec![None; questions.len()];
    for answer in answers {
        let index = questions
            .iter()
            .position(|q| q.id == answer.id)
            .ok_or_else(|| format!("no question {} was asked", answer.id))?;
        if given[index].is_some() {
            return Err(format!("question {} has more than one answer", answer.id));
        }
        given[index] = Some(cli_value(&questions[index], answer)?);
    }
    let mut out = Map::new();
    for (question, value) in questions.iter().zip(given) {
        let value = value.ok_or_else(|| format!("question {} has no answer", question.id))?;
        // The CLI keys answers by text, so two questions with one text get
        // one answer; differing answers for them cannot both be sent.
        if let Some(Value::String(earlier)) = out.get(&question.text)
            && *earlier != value
        {
            return Err(format!(
                "questions sharing the text {:?} need the same answer",
                question.text
            ));
        }
        out.insert(question.text.clone(), Value::String(value));
    }
    Ok(out)
}

/// One answer as the string the CLI takes.
fn cli_value(question: &Question, answer: &Answer) -> Result<String, String> {
    let ids: Vec<&String> = match (&answer.value, answer.own_words) {
        (AnswerValue::One(words), true) => return Ok(words.clone()),
        (AnswerValue::Many(_), true) => {
            return Err(format!(
                "the own words answering question {} are a list",
                question.id
            ));
        }
        (AnswerValue::One(id), false) => vec![id],
        (AnswerValue::Many(ids), false) if question.multi_select => ids.iter().collect(),
        (AnswerValue::Many(_), false) => {
            return Err(format!(
                "question {} takes one option, not a list",
                question.id
            ));
        }
    };
    let options = question.options.as_deref().unwrap_or_default();
    let mut chosen = Vec::new();
    for id in ids {
        let index = options
            .iter()
            .position(|o| &o.id == id)
            .ok_or_else(|| format!("question {} offers no option {id}", question.id))?;
        if chosen.contains(&index) {
            return Err(format!(
                "option {id} is chosen twice for question {}",
                question.id
            ));
        }
        chosen.push(index);
    }
    // The labels in the request's order, whatever order they were chosen in,
    // which is the order the projection reads them back in.
    chosen.sort_unstable();
    let labels: Vec<&str> = chosen.iter().map(|&i| options[i].label.as_str()).collect();
    Ok(labels.join(LABEL_SEPARATOR))
}

/// The answers the CLI's `answers` object gives `questions`, in question
/// order: each question takes the value under its text. A value equal to an
/// option's label is that option; for a multi-select question, a value that
/// is labels joined by ", " in the request's order is those options. Any
/// other value is the user's own words. A question whose text has no entry
/// gets no answer, and an entry naming no question is ignored.
pub fn from_cli(questions: &[Question], answers: &Map<String, Value>) -> Vec<Answer> {
    questions
        .iter()
        .filter_map(|question| {
            let value = answers.get(&question.text)?.as_str()?;
            let options = question.options.as_deref().unwrap_or_default();
            let labels: Vec<&str> = options.iter().map(|o| o.label.as_str()).collect();
            let chosen = if question.multi_select {
                joined_labels(&labels, value).map(|indices| {
                    AnswerValue::Many(indices.iter().map(|&i| options[i].id.clone()).collect())
                })
            } else {
                labels
                    .iter()
                    .position(|label| *label == value)
                    .map(|i| AnswerValue::One(options[i].id.clone()))
            };
            Some(match chosen {
                Some(value) => Answer {
                    id: question.id.clone(),
                    value,
                    own_words: false,
                },
                None => Answer {
                    id: question.id.clone(),
                    value: AnswerValue::One(value.to_string()),
                    own_words: true,
                },
            })
        })
        .collect()
}

/// The indices of the labels that, in order and joined by ", ", make `value`;
/// `None` when no such choice exists. No label chosen joins to "". A label
/// may hold ", " itself, so where two choices both fit, the one taking the
/// earliest labels first wins.
fn joined_labels(labels: &[&str], value: &str) -> Option<Vec<usize>> {
    if value.is_empty() {
        return Some(Vec::new());
    }
    // Depth-first over the labels in order: take label `i` when the rest of
    // `value` starts with it, then go on after the separator. `failed` keeps
    // the (position, next label) pairs already known to lead nowhere, so the
    // search stays small however the labels overlap.
    fn search(
        labels: &[&str],
        rest: &str,
        from: usize,
        failed: &mut std::collections::HashSet<(usize, usize)>,
        chosen: &mut Vec<usize>,
    ) -> bool {
        if failed.contains(&(rest.len(), from)) {
            return false;
        }
        for (i, label) in labels.iter().enumerate().skip(from) {
            let Some(after) = rest.strip_prefix(label) else {
                continue;
            };
            chosen.push(i);
            if after.is_empty() {
                return true;
            }
            if let Some(next) = after.strip_prefix(LABEL_SEPARATOR)
                && search(labels, next, i + 1, failed, chosen)
            {
                return true;
            }
            chosen.pop();
        }
        failed.insert((rest.len(), from));
        false
    }
    let mut chosen = Vec::new();
    search(labels, value, 0, &mut Default::default(), &mut chosen).then_some(chosen)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn question(text: &str, labels: &[&str], multi_select: bool) -> Question {
        let input = json!({ "questions": [{
            "question": text,
            "options": labels.iter().map(|l| json!({ "label": l })).collect::<Vec<_>>(),
            "multiSelect": multi_select,
        }] });
        asked(input.as_object().unwrap()).0.remove(0)
    }

    fn many(ids: &[&str]) -> AnswerValue {
        AnswerValue::Many(ids.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn labels_that_hold_the_separator_still_match() {
        let labels = ["Red, dark", "Red", "Blue"];
        assert_eq!(joined_labels(&labels, "Red, dark, Blue"), Some(vec![0, 2]));
        assert_eq!(joined_labels(&labels, "Red, Blue"), Some(vec![1, 2]));
        assert_eq!(
            joined_labels(&labels, "Blue, Red"),
            None,
            "the request's order only"
        );
        assert_eq!(joined_labels(&labels, "Red, dark"), Some(vec![0]));
        assert_eq!(joined_labels(&labels, ""), Some(vec![]));
        assert_eq!(joined_labels(&labels, "Green"), None);
    }

    #[test]
    fn an_answer_goes_to_the_cli_and_comes_back_the_same() {
        let questions = vec![
            question("Which?", &["A", "B"], false),
            Question {
                id: "1".into(),
                ..question("Which ones?", &["A", "B, c", "D"], true)
            },
            Question {
                id: "2".into(),
                ..question("Name?", &["Ann"], false)
            },
        ];
        let answers = vec![
            Answer {
                id: "1".into(),
                // Chosen out of order; sent in the request's order.
                value: many(&["2", "1"]),
                own_words: false,
            },
            Answer {
                id: "0".into(),
                value: AnswerValue::One("1".into()),
                own_words: false,
            },
            Answer {
                id: "2".into(),
                value: AnswerValue::One("Carol".into()),
                own_words: true,
            },
        ];
        let cli = to_cli(&questions, &answers).unwrap();
        assert_eq!(
            Value::Object(cli.clone()),
            json!({ "Which?": "B", "Which ones?": "B, c, D", "Name?": "Carol" })
        );
        let back = from_cli(&questions, &cli);
        assert_eq!(
            back.iter()
                .map(|a| (&a.id[..], &a.value, a.own_words))
                .collect::<Vec<_>>(),
            [
                ("0", &AnswerValue::One("1".into()), false),
                ("1", &many(&["1", "2"]), false),
                ("2", &AnswerValue::One("Carol".into()), true),
            ]
        );
    }

    #[test]
    fn answers_that_do_not_fit_the_questions_are_refused() {
        let questions = vec![
            question("Which?", &["A", "B"], false),
            Question {
                id: "1".into(),
                ..question("Which?", &["A", "B"], true)
            },
        ];
        let one = |id: &str, value: AnswerValue, own_words| Answer {
            id: id.into(),
            value,
            own_words,
        };
        let a = || AnswerValue::One("0".into());
        let refused = |answers: Vec<Answer>| to_cli(&questions, &answers).unwrap_err();
        assert!(refused(vec![one("0", a(), false)]).contains("no answer"));
        assert!(
            refused(vec![one("0", a(), false), one("0", a(), false)]).contains("more than one")
        );
        assert!(refused(vec![one("7", a(), false)]).contains("no question 7"));
        assert!(
            refused(vec![one("0", many(&["0"]), false), one("1", a(), false)])
                .contains("not a list")
        );
        assert!(
            refused(vec![
                one("0", AnswerValue::One("9".into()), false),
                one("1", a(), false)
            ])
            .contains("no option 9")
        );
        assert!(
            refused(vec![one("0", many(&["x"]), true), one("1", a(), false)])
                .contains("are a list")
        );
        assert!(
            refused(vec![
                one("0", a(), false),
                one("1", many(&["0", "0"]), false)
            ])
            .contains("twice")
        );
        // One text, one key: the two must agree.
        assert!(
            refused(vec![one("0", a(), false), one("1", many(&["1"]), false)])
                .contains("same answer")
        );
        let agreed = to_cli(
            &questions,
            &[one("0", a(), false), one("1", many(&["0"]), false)],
        )
        .unwrap();
        assert_eq!(Value::Object(agreed), json!({ "Which?": "A" }));
    }
}
