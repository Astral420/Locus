# Locus

Locus turns recorded conversations and supporting documents into a private, searchable meeting library.

## Language

**Meeting**:
A recorded session and its associated transcript, speakers, slides, summaries, and follow-up work. A meeting may be a business conversation or a lecture, online or in person.
_Avoid_: Using “meeting” to exclude lectures or microphone-only sessions.

**Recording**:
The captured audio and optional screen video belonging to a meeting.
_Avoid_: Using “video” for every recording.

**Capture source**:
One selected source of recorded content: system audio, microphone, or screen.

**Speaker**:
A voice distinguished within a meeting, with an anonymous label or a user-assigned name. An input device alone does not establish a person's identity.

**Meeting type**:
The business-meeting, lecture, or generic classification that determines the structure of a summary. Auto-detect is a selection mode rather than a content type.

**Summary revision**:
One generated account of a meeting based on a particular set of source results. Earlier revisions remain distinct from their replacements.

**Action item**:
A follow-up task extracted from a meeting, with any evidenced assignee and deadline and a user-controlled completion state.

**Document**:
An independently owned supporting file in the Knowledge Base, optionally linked to multiple meetings.
_Avoid_: Treating a linked document as owned by a meeting.

**Knowledge Base**:
The searchable collection of meeting content and independently uploaded documents.

**Chat scope**:
The source boundary of a conversation thread: one meeting with its linked documents, all meetings, documents only, or everything.

**Citation**:
A reference from an answer or generated claim to supporting meeting content or a document location.

**Configured provider**:
A provider made available through saved settings or credentials.

**Selected provider**:
The provider explicitly chosen to process a generation request. Selection authorizes use of that destination for the disclosed request content.
_Avoid_: Treating configured credentials as provider selection.

**Model setup**:
Provisioning the local generation and embedding models required for offline summarization and semantic search.

**Outdated result**:
A previously successful result whose source content has since changed. It remains distinguishable from a current result or a failed replacement attempt.

