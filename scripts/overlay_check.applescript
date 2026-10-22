-- The overlay check.
--
-- This drives a real app, with real text, and asks the question the M0 gate
-- asks: does Elocutio underline the mistake and offer a card?
--
-- The check is deliberately simple. It types a sentence with a deliberate
-- typo into an app and reports what it could observe. It cannot prove the
-- card was drawn or that the replacement is correct, so it reports what it
-- saw and leaves the rest to a human.
--
-- Usage:
--   osascript scripts/overlay_check.applescript "TextEdit"
--   osascript scripts/overlay_check.applescript "Mail"
--   osascript scripts/overlay_check.applescript "Google Chrome" "https://example.test/"
--
-- The first argument is the app name. The second, for a browser, is a page
-- to open first. Mail needs a compose window, so the script makes one.
--
-- The output is one line per fact, so it can be read by a person or parsed.

on run argv
	if (count of argv) is 0 then
		return "usage: osascript scripts/overlay_check.applescript <app name> [url]"
	end if

	set appName to item 1 of argv
	set marker to "tehre"
	set sentence to "This is " & marker & " test."
	set notes to ""

	-- A browser has to be given somewhere to type.
	if (count of argv) > 1 then
		tell application appName
			activate
			delay 1
			open location (item 2 of argv)
		end tell
		delay 2
		set notes to notes & "page: " & (item 2 of argv) & linefeed
	end if

	tell application appName
		activate
		delay 1
	end tell

	-- Type into the app the way a person would, with the keyboard, so the
	-- Accessibility layer sees a real key event rather than a scripted
	-- value change.
	tell application "System Events"
		tell process appName
			set frontmost to true
			delay 0.5
			-- Mail needs a compose window before there is a field to type in.
			try
				tell (menu item "New Message" of menu 1 of menu bar item "Message" of menu bar 1) to click
				delay 2
				set notes to notes & "compose: opened" & linefeed
			end try
			delay 0.5
			keystroke sentence
			delay 3
		end tell
	end tell

	-- The highlighter draws its overlay in its own windows. Counting the
	-- app's windows is a rough proxy: it says something was drawn, not what.
	set overlayWindows to 0
	try
		tell application "System Events"
			tell process appName
				set overlayWindows to count of windows
			end tell
		end tell
	end try

	return "app: " & appName & linefeed & notes & "windows: " & overlayWindows
end run
