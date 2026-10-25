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
		if type(option) ~= "string" then
			error("ui.choices options must all be strings")
		end
		if option == props.value then
			selected = true
		end
	end
	if not selected then
		error("ui.choices value must match one of its options")
	end
	if type(props.on_change) ~= "function" then
		error("ui.choices requires on_change")
	end
	local title = props.title
	local description = props.description
	local value = props.value
	local message = (props.reply_prefix or "") .. value .. (props.reply_suffix or "")
	if #message == 0 or #message > 8192 then
		error("ui.choices reply message must contain 1 to 8192 bytes")
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
	table.insert(view, ui.reply {
		label = props.submit_label or "Continue",
		message = message,
		primary = props.primary ~= false,
	})
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

return ui
