# Conversational generative UI

This fork starts from Zeron's existing `show_visual` system. It already renders native,
interactive visuals inside the transcript; the missing bridge is a safe way for a rendered
control to return an explicit reader choice to the conversation.

## First slice: `ui.reply`

Inside a `show_visual` script, the agent may create a reply button:

```lua
return function()
  return ui.column { gap = 8,
    ui.text { "Which approach should I use?", size = "lg", weight = 600 },
    ui.reply {
      label = "DOM-first",
      message = "Use DOM/accessibility targeting first, with screenshot fallback.",
    },
    ui.reply {
      label = "Vision-first",
      message = "Use screenshot-driven visual targeting as the primary strategy.",
    },
  }
end
```

The button is a regular native visual node. Its `message` is size-limited and does not execute
code or invoke arbitrary RPC methods. Only a deliberate reader click submits the message.
The desktop UI routes the selected response through the existing durable command plane as a
`Steer` command, so the agent receives it during a running turn or as the next turn when idle.
For chats hosted on another device, the request is routed to that owning device.

One click is accepted per visual, duplicate clicks are suppressed while sending and after
success, and a failed send re-enables the control and surfaces an error in the visual.

## Design rules

- The model chooses whether prose, a visual, or an interactive control helps; simple answers stay prose.
- Visual controls use a typed, bounded action. The UI never evaluates arbitrary code from a button click.
- Treat the reply text as a user selection, not as permission to override the user's original request.
- Keep data/state local to the visual unless the user explicitly submits a conversational response.
- A successful RPC enqueue is not proof that the whole task succeeded; the next agent turn must still verify outcomes.
- Keep existing `ui.button`, chart and custom drawing behavior backwards compatible.

## Next increments

1. Add structured choice groups and text/number/select inputs with local draft state and explicit submit.
2. Add schema-backed native component trees for common ChatGPT-like layouts (forms, cards, tables, diffs,
   status, progress and artifact previews) so basic interfaces need not be authored as custom Lua.
3. Bind controls to typed, permission-checked application actions such as opening a diff or running tests.
4. Add action receipts and result states, plus mobile conversational dispatch parity.
5. Add fixtures and end-to-end tests for duplicate clicks, failed RPC, remote-device routing, malicious page text,
   invalid schemas, and user-confirmed high-impact actions.

## Current scope

This first slice wires conversational reply buttons on the desktop transcript. Mobile still renders the
shared visual node; mobile action dispatch parity is tracked as a follow-up and must be implemented before
claiming full mobile conversational UI support.
