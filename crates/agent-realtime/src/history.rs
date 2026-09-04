use agent_protocol::{
    BemItemPresentation, RealtimeEvent, RealtimeItem, RealtimeItemContent, RealtimeSessionOutcome,
    RealtimeTranscriptRole,
};
use uuid::Uuid;

#[derive(Debug)]
struct Segment {
    id: String,
    text: String,
}

/// Reduces transient provider events into append-only realtime history items.
#[derive(Debug, Default)]
pub struct RealtimeHistory {
    session_id: Option<String>,
    user: Option<Segment>,
    assistant: Option<Segment>,
    first_active_role: Option<RealtimeTranscriptRole>,
    failed: bool,
}

impl RealtimeHistory {
    pub fn start(&mut self, session_id: impl Into<String>) -> Vec<RealtimeItem> {
        let session_id = session_id.into();
        let mut items = self.close_with(RealtimeSessionOutcome::Ended);
        self.session_id = Some(session_id.clone());
        self.failed = false;
        items.push(item(
            &session_id,
            RealtimeItemContent::RealtimeSessionStarted,
        ));
        items
    }

    pub fn observe(&mut self, event: &RealtimeEvent) -> Vec<RealtimeItem> {
        match event {
            RealtimeEvent::InputTranscriptDelta(delta) => {
                self.push(RealtimeTranscriptRole::User, &delta.delta);
                Vec::new()
            }
            RealtimeEvent::OutputTranscriptDelta(delta) => {
                self.push(RealtimeTranscriptRole::Assistant, &delta.delta);
                Vec::new()
            }
            RealtimeEvent::InputTranscriptDone(done) => {
                self.finish(RealtimeTranscriptRole::User, &done.text)
            }
            RealtimeEvent::OutputTranscriptDone(done) => {
                self.finish(RealtimeTranscriptRole::Assistant, &done.text)
            }
            RealtimeEvent::Error(_) => {
                self.failed = true;
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    pub fn promote(
        &mut self,
        turn_id: impl Into<String>,
        item_id: impl Into<String>,
        presentation: BemItemPresentation,
    ) -> Vec<RealtimeItem> {
        let Some(session_id) = self.session_id.clone() else {
            return Vec::new();
        };
        let mut items = self.seal_all();
        items.push(item(
            &session_id,
            RealtimeItemContent::BemItemPromoted {
                turn_id: turn_id.into(),
                item_id: item_id.into(),
                presentation,
            },
        ));
        items
    }

    pub fn close(&mut self) -> Vec<RealtimeItem> {
        let outcome = if self.failed {
            RealtimeSessionOutcome::Failed
        } else {
            RealtimeSessionOutcome::Ended
        };
        self.close_with(outcome)
    }

    pub fn close_discarding_tail(&mut self) -> Vec<RealtimeItem> {
        self.user = None;
        self.assistant = None;
        self.close()
    }

    fn push(&mut self, role: RealtimeTranscriptRole, text: &str) {
        if self.session_id.is_none() || text.is_empty() {
            return;
        }
        self.first_active_role.get_or_insert(role);
        let slot = match role {
            RealtimeTranscriptRole::User => &mut self.user,
            RealtimeTranscriptRole::Assistant => &mut self.assistant,
        };
        slot.get_or_insert_with(|| Segment {
            id: Uuid::new_v4().to_string(),
            text: String::new(),
        })
        .text
        .push_str(text);
    }

    fn finish(&mut self, role: RealtimeTranscriptRole, authoritative: &str) -> Vec<RealtimeItem> {
        let Some(session_id) = self.session_id.clone() else {
            return Vec::new();
        };
        let slot = match role {
            RealtimeTranscriptRole::User => &mut self.user,
            RealtimeTranscriptRole::Assistant => &mut self.assistant,
        };
        let segment = slot.take();
        if self.first_active_role == Some(role) {
            self.first_active_role = match role {
                RealtimeTranscriptRole::User => self
                    .assistant
                    .as_ref()
                    .map(|_| RealtimeTranscriptRole::Assistant),
                RealtimeTranscriptRole::Assistant => {
                    self.user.as_ref().map(|_| RealtimeTranscriptRole::User)
                }
            };
        }
        let (id, buffered) = segment
            .map(|value| (value.id, value.text))
            .unwrap_or_else(|| (Uuid::new_v4().to_string(), String::new()));
        let text = if authoritative.is_empty() {
            buffered
        } else {
            authoritative.to_string()
        };
        if text.is_empty() {
            return Vec::new();
        }
        vec![RealtimeItem {
            id,
            realtime_session_id: session_id,
            content: RealtimeItemContent::TranscriptSegment { role, text },
        }]
    }

    fn seal_all(&mut self) -> Vec<RealtimeItem> {
        let roles = match self.first_active_role {
            Some(RealtimeTranscriptRole::Assistant) => [
                RealtimeTranscriptRole::Assistant,
                RealtimeTranscriptRole::User,
            ],
            Some(RealtimeTranscriptRole::User) | None => [
                RealtimeTranscriptRole::User,
                RealtimeTranscriptRole::Assistant,
            ],
        };
        let mut items = Vec::new();
        for role in roles {
            items.extend(self.finish(role, ""));
        }
        items
    }

    fn close_with(&mut self, outcome: RealtimeSessionOutcome) -> Vec<RealtimeItem> {
        let Some(session_id) = self.session_id.clone() else {
            return Vec::new();
        };
        let mut items = self.seal_all();
        self.session_id = None;
        self.first_active_role = None;
        items.push(item(
            &session_id,
            RealtimeItemContent::RealtimeSessionClosed { outcome },
        ));
        self.failed = false;
        items
    }
}

fn item(session_id: &str, content: RealtimeItemContent) -> RealtimeItem {
    RealtimeItem {
        id: Uuid::new_v4().to_string(),
        realtime_session_id: session_id.to_string(),
        content,
    }
}

#[cfg(test)]
mod tests {
    use agent_protocol::{RealtimeTranscriptDelta, RealtimeTranscriptDone};

    use super::*;

    #[test]
    fn stores_only_completed_transcript_segments() {
        let mut history = RealtimeHistory::default();
        assert_eq!(history.start("session-1").len(), 1);
        assert!(history
            .observe(&RealtimeEvent::InputTranscriptDelta(
                RealtimeTranscriptDelta {
                    delta: "hel".into()
                }
            ))
            .is_empty());
        let items = history.observe(&RealtimeEvent::InputTranscriptDone(
            RealtimeTranscriptDone {
                text: "hello".into(),
            },
        ));
        assert!(matches!(
            &items[0].content,
            RealtimeItemContent::TranscriptSegment { role: RealtimeTranscriptRole::User, text }
                if text == "hello"
        ));
    }

    #[test]
    fn error_marks_closed_session_failed() {
        let mut history = RealtimeHistory::default();
        history.start("session-1");
        history.observe(&RealtimeEvent::Error("lost".into()));
        let items = history.close();
        assert!(matches!(
            items.last().map(|item| &item.content),
            Some(RealtimeItemContent::RealtimeSessionClosed {
                outcome: RealtimeSessionOutcome::Failed
            })
        ));
    }

    #[test]
    fn configured_discard_drops_partial_tail_before_close() {
        let mut history = RealtimeHistory::default();
        history.start("session-1");
        history.observe(&RealtimeEvent::OutputTranscriptDelta(
            RealtimeTranscriptDelta {
                delta: "partial".into(),
            },
        ));
        let items = history.close_discarding_tail();
        assert_eq!(items.len(), 1);
        assert!(matches!(
            items[0].content,
            RealtimeItemContent::RealtimeSessionClosed { .. }
        ));
    }

    #[test]
    fn starting_a_new_session_seals_the_previous_session_in_order() {
        let mut history = RealtimeHistory::default();
        history.start("session-1");
        history.observe(&RealtimeEvent::InputTranscriptDelta(
            RealtimeTranscriptDelta {
                delta: "first".into(),
            },
        ));

        let boundary = history.start("session-2");
        assert!(matches!(
            &boundary[0].content,
            RealtimeItemContent::TranscriptSegment {
                role: RealtimeTranscriptRole::User,
                text,
            } if text == "first"
        ));
        assert!(matches!(
            boundary[1].content,
            RealtimeItemContent::RealtimeSessionClosed {
                outcome: RealtimeSessionOutcome::Ended
            }
        ));
        assert!(matches!(
            boundary[2].content,
            RealtimeItemContent::RealtimeSessionStarted
        ));
        assert_eq!(boundary[0].realtime_session_id, "session-1");
        assert_eq!(boundary[2].realtime_session_id, "session-2");
    }

    #[test]
    fn handoff_promotion_seals_transcripts_before_the_promoted_item() {
        let mut history = RealtimeHistory::default();
        history.start("session-1");
        history.observe(&RealtimeEvent::InputTranscriptDelta(
            RealtimeTranscriptDelta {
                delta: "please continue".into(),
            },
        ));
        history.observe(&RealtimeEvent::OutputTranscriptDelta(
            RealtimeTranscriptDelta {
                delta: "working".into(),
            },
        ));

        let items = history.promote(
            "turn-1",
            "item-1",
            agent_protocol::BemItemPresentation::WholeItem,
        );

        assert!(matches!(
            &items[0].content,
            RealtimeItemContent::TranscriptSegment {
                role: RealtimeTranscriptRole::User,
                text,
            } if text == "please continue"
        ));
        assert!(matches!(
            &items[1].content,
            RealtimeItemContent::TranscriptSegment {
                role: RealtimeTranscriptRole::Assistant,
                text,
            } if text == "working"
        ));
        assert!(matches!(
            &items[2].content,
            RealtimeItemContent::BemItemPromoted { turn_id, item_id, .. }
                if turn_id == "turn-1" && item_id == "item-1"
        ));
    }
}
