# showcase.tape — tighter capture for a ratatui showcase submission.
#
#   vhs showcase.tape    # writes showcase.gif in the repo root
#
# Deliberately different from demo.tape. The showcase opens inside the app on
# the interactive search prompt, types the idea there, shows the spinner
# briefly, then hides the rest of the load wait.
# See https://ratatui.rs/recipes/apps/submitting-to-the-showcase/
#
# Pre-warm Ollama before recording (`ollama serve` + one throwaway run) so the
# verdict is real LLM text, not the --fast similarity fallback.

Output showcase.gif

Set Shell "bash"
Set FontSize 18
Set Width 1200
Set Height 720
Set Padding 24
Set Theme "Dracula"

# Launch off-screen so the first frame is the app, not the shell.
Hide
Type "./target/release/patent"
Enter
Sleep 2s
Show

# Search from inside the TUI.
Sleep 800ms
Type@45ms "fuzzy finder for searching files and text in the terminal"
Sleep 600ms
Enter
Sleep 1500ms

# Hide the rest of the load wait (search + ranking + verdict).
# Bump the sleep if your machine or Ollama model is slower.
Hide
Sleep 18s
Show

# Hero frame: ranked matches + verdict. Hold so it still reads when scaled down.
Sleep 3s

# Screenshot for showcase thumbnail (scales better than gif on small screens).
Screenshot showcase.png

# A little calm motion through the matches.
Down@350ms 4
Sleep 800ms

# Detail popup — shows description, popularity, URL.
Enter
Sleep 4500ms
Escape
Sleep 1200ms

# New search from the results screen, back to the prompt.
Type "n"
Sleep 1500ms
