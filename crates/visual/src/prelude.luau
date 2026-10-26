-- The `ui` library every visual script sees. Constructors only tag their
-- props table; Zeron validates and renders the tree natively.
local ui = {}

local function constructor(kind)
	return function(props)
		if type(props) ~= "table" or rawget(props, "__kind") ~= nil then
			props = { props }
		end
		props.__kind = kind
		return props
	end
end

for _, kind in
	{
		"column",
		"row",
		"grid",
		"card",
		"text",
		"span",
		"rule",
		"space",
		"stat",
		"table",
		"chart",
		"canvas",
		"scene",
		"shader",
		"button",
		"tabs",
		"toggle",
		"slider",
		"link",
	}
do
	ui[kind] = constructor(kind)
end

-- A theme token at an opacity.
function ui.color(token, alpha)
	return { __color = true, token = token, alpha = alpha }
end

-- A blend of two theme tokens, `t` of the way from `a` to `b`.
function ui.mix(a, b, t)
	return { __color = true, token = a, mix = b, t = t }
end

-- The `i`th categorical series color (1-based, cycling through six).
function ui.series(i)
	return "chart" .. ((i - 1) % 6 + 1)
end

-- The table your handlers change. Zeron renders the view again after every
-- handler, so plain assignment is enough.
function ui.state(initial)
	return initial or {}
end

-- A linear map from the domain [d0, d1] onto the range [r0, r1].
function ui.scale(d0, d1, r0, r1)
	local span = d1 - d0
	if span == 0 then
		span = 1
	end
	return function(v)
		return r0 + (v - d0) / span * (r1 - r0)
	end
end

function ui.clamp(v, lo, hi)
	return math.max(lo, math.min(hi, v))
end

function ui.lerp(a, b, t)
	return a + (b - a) * t
end

function ui.map(list, f)
	local out = {}
	for i, v in ipairs(list) do
		out[i] = f(v, i)
	end
	return out
end

function ui.range(a, b, step)
	if b == nil then
		a, b = 1, a
	end
	step = step or 1
	local out = {}
	for v = a, b, step do
		table.insert(out, v)
	end
	return out
end

-- A conversational reply is still a native button, but carries a bounded
-- message that the host submits to the current chat only after the reader clicks.
function ui.reply(props)
	if type(props) ~= "table" then
		error("ui.reply expects a props table")
	end
	if type(props.message) ~= "string" then
		error("ui.reply requires a message string")
	end
	if #props.message == 0 or #props.message > 8192 or not string.match(props.message, "%S") then
		error("ui.reply message must contain 1 to 8192 non-whitespace bytes")
	end
	props.__kind = "button"
	props.reply = props.message
	props.message = nil
	props.on_click = props.on_click or function() end
	return props
end

-- A native choice group: tabs capture a draft selection, then one reply button
-- sends the selected value to the owning conversation. The state stays in the
-- agent's closure, so re-rendering after a tab change preserves the selection.
function ui.choices(props)
	if type(props) ~= "table" then
		error("ui.choices expects a props table")
	end
	if type(props.options) ~= "table" or #props.options == 0 then
		error("ui.choices requires a non-empty options array")
	end
	if type(props.value) ~= "string" then
		error("ui.choices requires value to be a string")
	end
	local selected = false
	for _, option in ipairs(props.options) do
		if type(option) ~= "string" or not string.match(option, "%S") then
			error("ui.choices options must be non-empty strings")
		end
		if option == props.value then
			selected = true
		end
	end
	-- An empty value is an intentional unselected state. Non-empty invalid
	-- values remain errors so stale state cannot silently select another option.
	if not selected and props.value ~= "" then
		error("ui.choices value must be empty or match one of its options")
	end
	if type(props.on_change) ~= "function" then
		error("ui.choices requires on_change")
	end
	local title = props.title
	local description = props.description
	if title ~= nil and type(title) ~= "string" then
		error("ui.choices title must be a string")
	end
	if description ~= nil and type(description) ~= "string" then
		error("ui.choices description must be a string")
	end
	local value = props.value
	if props.reply_prefix ~= nil and type(props.reply_prefix) ~= "string" then
		error("ui.choices reply_prefix must be a string")
	end
	if props.reply_suffix ~= nil and type(props.reply_suffix) ~= "string" then
		error("ui.choices reply_suffix must be a string")
	end
	-- The visual itself may not be present in the model's next-turn text context.
	-- Send the complete decision context with the chosen answer, not just its label.
	local answer = (props.reply_prefix or "") .. value .. (props.reply_suffix or "")
	local context = {}
	if title and string.match(title, "%S") then
		table.insert(context, "Question: " .. title)
	end
	if description and string.match(description, "%S") then
		table.insert(context, "Context: " .. description)
	end
	table.insert(context, "Options: " .. table.concat(props.options, " | "))
	table.insert(context, "Selected answer: " .. answer)
	local message = table.concat(context, "\n")
	if selected and (#message == 0 or #message > 8192 or not string.match(message, "%S")) then
		error("ui.choices reply message must contain 1 to 8192 non-whitespace bytes including its context")
	end
	local view = { __kind = "column", gap = props.gap or 12 }
	if title then
		table.insert(view, ui.text { title, size = "lg", weight = 600 })
	end
	if description then
		table.insert(view, ui.text { description, color = "muted" })
	end
	table.insert(view, ui.tabs {
		options = props.options,
		value = value,
		on_change = props.on_change,
	})
	if selected then
		table.insert(view, ui.reply {
			label = props.submit_label or "Continue",
			message = message,
			primary = props.primary ~= false,
		})
	else
		table.insert(view, ui.text {
			"Choose an option to continue.",
			size = "sm",
			color = "muted",
		})
	end
	return view
end


-- High-level conversational components. They expand to ordinary native nodes,
-- so they inherit Zeron's layout, theme, accessibility, and sandbox limits.
local function check_component_props(props, name, allowed)
	for key in pairs(props) do
		if type(key) == "string" and not allowed[key] then
			error(name .. " has no '" .. key .. "'")
		end
	end
end

local function tone_color(tone, default)
	local colors = {
		info = "accent",
		success = "success",
		warning = "warning",
		error = "danger",
		neutral = "muted",
	}
	local color = colors[tone or default]
	if not color then
		error("tone must be info, success, warning, error, or neutral")
	end
	return color
end

-- A compact state row for work that is waiting, running, complete, or blocked.
function ui.status(props)
	if type(props) ~= "table" then
		error("ui.status expects a props table")
	end
	check_component_props(props, "ui.status", { label = true, detail = true, tone = true })
	if type(props.label) ~= "string" or not string.match(props.label, "%S") then
		error("ui.status requires a non-empty label")
	end
	if props.detail ~= nil and type(props.detail) ~= "string" then
		error("ui.status detail must be a string")
	end
	local color = tone_color(props.tone, "info")
	local body = ui.column {
		gap = 2,
		ui.text { props.label, size = "body", weight = 600 },
	}
	if props.detail and props.detail ~= "" then
		table.insert(body, ui.text { props.detail, size = "sm", color = "muted" })
	end
	return ui.row {
		gap = 8,
		align = "center",
		ui.text { "●", size = "sm", color = color },
		body,
	}
end

-- A bounded progress bar made of native text spans. It remains readable even
-- in narrow transcript columns and reports the clamped percentage.
function ui.progress(props)
	if type(props) ~= "table" then
		error("ui.progress expects a props table")
	end
	check_component_props(props, "ui.progress", {
		label = true, value = true, min = true, max = true, tone = true,
		segments = true, show_value = true,
	})
	local value = props.value
	local min = props.min or 0
	local max = props.max or 100
	if type(value) ~= "number" or value ~= value or value == math.huge or value == -math.huge then
		error("ui.progress value must be a finite number")
	end
	if type(min) ~= "number" or type(max) ~= "number"
		or min ~= min or max ~= max or min == math.huge or max == math.huge
		or min == -math.huge or max == -math.huge or max <= min then
		error("ui.progress requires finite min and max values, with max greater than min")
	end
	if props.label ~= nil and type(props.label) ~= "string" then
		error("ui.progress label must be a string")
	end
	if props.show_value ~= nil and type(props.show_value) ~= "boolean" then
		error("ui.progress show_value must be true or false")
	end
	local segments = props.segments or 12
	if type(segments) ~= "number" then
		error("ui.progress segments must be a number")
	end
	segments = math.floor(ui.clamp(segments, 6, 24))
	local ratio = ui.clamp((value - min) / (max - min), 0, 1)
	local filled = math.floor(ratio * segments + 0.5)
	local color = tone_color(props.tone, "info")
	local header = ui.row { gap = 12, align = "center", justify = "between" }
	if props.label and props.label ~= "" then
		table.insert(header, ui.text { props.label, size = "sm", color = "muted" })
	end
	if props.show_value ~= false then
		table.insert(header, ui.text {
			tostring(math.floor(ratio * 100 + 0.5)) .. "%",
			size = "sm", weight = 600, mono = true,
		})
	end
	local bar = ui.text {
		ui.span { string.rep("█", filled), color = color, mono = true },
		ui.span { string.rep("░", segments - filled), color = "border", mono = true },
		size = "sm",
	}
	return ui.column { gap = 2, header, bar }
end

-- A deliberately small, data-only diff preview. It never runs or applies code.
-- The view caps each side to 40 lines; omitted counts are called out explicitly.
function ui.diff(props)
	if type(props) ~= "table" then
		error("ui.diff expects a props table")
	end
	check_component_props(props, "ui.diff", {
		file = true, language = true, removed = true, added = true,
	})
	if props.file ~= nil and type(props.file) ~= "string" then
		error("ui.diff file must be a string")
	end
	if props.language ~= nil and type(props.language) ~= "string" then
		error("ui.diff language must be a string")
	end
	local removed = props.removed or {}
	local added = props.added or {}
	if type(removed) ~= "table" or type(added) ~= "table" then
		error("ui.diff removed and added must be arrays of strings")
	end
	local card = {
		title = props.file or "Code changes",
		description = props.language,
		gap = 4,
	}
	local shown_removed = math.min(#removed, 40)
	local shown_added = math.min(#added, 40)
	for i = 1, shown_removed do
		local line = removed[i]
		if type(line) ~= "string" then
			error("ui.diff removed entries must be strings")
		end
		table.insert(card, ui.row {
			gap = 8, align = "start",
			ui.text { "−", color = "danger", mono = true, weight = 600, size = "sm" },
			ui.text { line, color = "danger", mono = true, size = "sm" },
		})
	end
	for i = 1, shown_added do
		local line = added[i]
		if type(line) ~= "string" then
			error("ui.diff added entries must be strings")
		end
		table.insert(card, ui.row {
			gap = 8, align = "start",
			ui.text { "+", color = "success", mono = true, weight = 600, size = "sm" },
			ui.text { line, color = "success", mono = true, size = "sm" },
		})
	end
	if #removed == 0 and #added == 0 then
		table.insert(card, ui.text { "No line changes", color = "muted", size = "sm" })
	end
	if #removed > shown_removed or #added > shown_added then
		table.insert(card, ui.text {
			"Preview truncated (" .. tostring(#removed - shown_removed) ..
			" removed and " .. tostring(#added - shown_added) .. " added lines omitted).",
			color = "muted", size = "xs",
		})
	end
	return ui.card(card)
end


-- Collect the current values from supported native controls, including controls
-- nested in layout nodes. This is sent with the form reply because the next agent
-- turn cannot safely infer the live visual's state from prose alone.
local function collect_form_values(node, values, depth)
	if type(node) ~= "table" or depth > 12 then
		return
	end
	local kind = node.__kind
	if kind == "tabs" then
		local label = node.context_label or "Selection"
		local value = node.value
		if value == nil then
			value = type(node.options) == "table" and node.options[1] or nil
		end
		local selected = value == nil or value == "" and "(no selection)" or tostring(value)
		local options = type(node.options) == "table" and table.concat(node.options, " | ") or ""
		table.insert(values, label .. ": " .. selected .. (options ~= "" and " (options: " .. options .. ")" or ""))
		-- context_label is metadata for ui.form, not a native tabs property.
		node.context_label = nil
	elseif kind == "toggle" then
		table.insert(values, (node.label or "Toggle") .. ": " .. (node.value and "On" or "Off"))
	elseif kind == "slider" then
		local number = node.value
		if number == nil then number = node.min or 0 end
		local label = node.label or "Value"
		table.insert(values, label .. ": " .. tostring(number) .. (node.unit or ""))
	end
	for _, child in ipairs(node) do
		collect_form_values(child, values, depth + 1)
	end
end

-- A compact, validated form composition. Inputs stay as drafts until the
-- explicit reply button is clicked; callers must compute validity from state.
-- This component never executes application actions itself.
function ui.form(props)
	if type(props) ~= "table" then
		error("ui.form expects a props table")
	end
	check_component_props(props, "ui.form", {
		title = true, description = true, fields = true, valid = true,
		validation_message = true, submit_label = true, message = true,
		primary = true, gap = true,
	})
	if props.title ~= nil and type(props.title) ~= "string" then
		error("ui.form title must be a string")
	end
	if props.description ~= nil and type(props.description) ~= "string" then
		error("ui.form description must be a string")
	end
	if type(props.fields) ~= "table" or #props.fields == 0 or #props.fields > 12 then
		error("ui.form requires between 1 and 12 fields")
	end
	for i, field in ipairs(props.fields) do
		if type(field) ~= "table" then
			error("ui.form field " .. tostring(i) .. " must be a native UI component")
		end
		-- Optional label for tabs, which intentionally have no visible label of their own.
		if field.context_label ~= nil and type(field.context_label) ~= "string" then
			error("ui.form field " .. tostring(i) .. " context_label must be a string")
		end
	end
	if type(props.valid) ~= "boolean" then
		error("ui.form requires an explicit valid boolean")
	end
	if type(props.message) ~= "string" or #props.message == 0
		or #props.message > 8192 or not string.match(props.message, "%S") then
		error("ui.form message must contain 1 to 8192 non-whitespace bytes")
	end
	if props.validation_message ~= nil and type(props.validation_message) ~= "string" then
		error("ui.form validation_message must be a string")
	end
	if props.primary ~= nil and type(props.primary) ~= "boolean" then
		error("ui.form primary must be true or false")
	end
	local card = ui.card {
		title = props.title,
		description = props.description,
		gap = props.gap or 10,
	}
	for _, field in ipairs(props.fields) do
		table.insert(card, field)
	end
	if props.valid then
		local values = {}
		for _, field in ipairs(props.fields) do
			collect_form_values(field, values, 0)
		end
		local context = {}
		if props.title and string.match(props.title, "%S") then
			table.insert(context, "Form: " .. props.title)
		end
		if props.description and string.match(props.description, "%S") then
			table.insert(context, "Context: " .. props.description)
		end
		table.insert(context, props.message)
		if #values > 0 then
			table.insert(context, "Submitted control values:\n" .. table.concat(values, "\n"))
		end
		local message = table.concat(context, "\n\n")
		if #message > 8192 then
			error("ui.form submitted context must not exceed 8192 bytes")
		end
		table.insert(card, ui.reply {
			label = props.submit_label or "Submit",
			message = message,
			primary = props.primary ~= false,
		})
	else
		table.insert(card, ui.status {
			label = "More information needed",
			detail = props.validation_message or "Complete the fields before submitting.",
			tone = "warning",
		})
	end
	return card
end

-- A bounded task/checklist timeline. State is presentation only and should
-- reflect observed work, not a prediction that a step has already succeeded.
function ui.steps(props)
	if type(props) ~= "table" then
		error("ui.steps expects a props table")
	end
	check_component_props(props, "ui.steps", {
		title = true, description = true, steps = true, gap = true,
	})
	if props.title ~= nil and type(props.title) ~= "string" then
		error("ui.steps title must be a string")
	end
	if props.description ~= nil and type(props.description) ~= "string" then
		error("ui.steps description must be a string")
	end
	if type(props.steps) ~= "table" or #props.steps == 0 or #props.steps > 12 then
		error("ui.steps requires between 1 and 12 steps")
	end
	local symbols = {
		done = { "✓", "success" },
		active = { "●", "accent" },
		pending = { "○", "muted" },
		blocked = { "!", "danger" },
	}
	local card = ui.card {
		title = props.title or "Progress",
		description = props.description,
		gap = props.gap or 8,
	}
	for i, step in ipairs(props.steps) do
		if type(step) ~= "table" then
			error("ui.steps entry " .. tostring(i) .. " must be a table")
		end
		if type(step.label) ~= "string" or not string.match(step.label, "%S") then
			error("ui.steps entry " .. tostring(i) .. " needs a non-empty label")
		end
		if step.detail ~= nil and type(step.detail) ~= "string" then
			error("ui.steps detail must be a string")
		end
		local state = step.state or "pending"
		local marker = symbols[state]
		if not marker then
			error("ui.steps state must be done, active, pending, or blocked")
		end
		local content = ui.column {
			gap = 2,
			ui.text { step.label, size = "body", weight = state == "active" and 600 or 500 },
		}
		if step.detail and step.detail ~= "" then
			table.insert(content, ui.text { step.detail, size = "sm", color = "muted" })
		end
		table.insert(card, ui.row {
			gap = 8,
			align = "start",
			ui.text { marker[1], size = "sm", color = marker[2], mono = true, weight = 600 },
			content,
		})
	end
	return card
end

return ui
