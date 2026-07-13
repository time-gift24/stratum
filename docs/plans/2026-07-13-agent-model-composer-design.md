# Agent and Model Composer Design

## Goal

Make a new Longzhong conversation immediately usable by selecting a default
Agent template as soon as configuration metadata loads. Show the template's
default model as a persistent, compact control on the left side of the
composer. Changing Agent always starts a new, uncreated conversation and
never mutates the active Agent session.

## Interaction

1. The frontend loads Agent templates and model descriptors together.
2. If no conversation is active, it selects the first available Agent template
   once metadata is ready. The template order remains the backend's source of
   truth for the default.
3. The composer control in the left tool area displays the Provider parsed from
   the model identifier and the selected model name, for example
   `DeepSeek · deepseek-v4-flash`.
4. Opening that control presents a flat Agent radio group. Selecting a
   different Agent clears the current conversation selection, clears pending
   model overrides, and applies the new template's `model_config` immediately.
   The next submitted message creates a new Agent session with that template.
5. With an existing session, the same Agent group remains available. Selecting
   an Agent performs the same new-conversation transition instead of changing
   the model of the active session.
6. Existing-session model and thinking controls remain available only for the
   active session. They retain the current next-message semantics and stay
   disabled while a turn is running.

## Data Flow and Boundaries

- `useAgentConversation` owns default-template selection and the transition
  from an existing Agent session to a new, uncreated conversation.
- `ModelConfigMenu` is presentation and invokes an explicit Agent-selection
  callback. It does not infer defaults itself.
- The template `model_config` is always used for a new conversation. A model
  override from a prior session is never carried across Agent selection.
- The API has no separate Provider field. The UI derives it from the portion of
  the model identifier before the first colon and falls back to the model name
  when no prefix is present.
- No changes are required to the reusable UI primitives under
  `app/components/ui/*` or `app/components/ai-elements/*`.

## States

- Metadata loading: the configuration control states that configuration is
  loading and is unavailable.
- Metadata failure or no templates: preserve the existing inline error path
  and prevent creating a conversation without a template.
- Default state: first template selected, template default model shown, first
  prompt creates the Agent session.
- Existing session: active model is displayed; Agent selection creates a fresh
  conversation configuration; model and thinking changes affect only later
  messages in the current session.

## Verification

The web workspace policy forbids adding or maintaining frontend test files.
Verification will therefore use the existing typecheck, test, and production
build commands, followed by a manual browser check for initial defaults and
Agent switching behavior.
