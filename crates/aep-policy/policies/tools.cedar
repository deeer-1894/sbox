// Default-deny: only the listed tools may be called. Anything else is denied.
permit(
  principal,
  action == Action::"CallTool",
  resource
)
when { [Tool::"echo", Tool::"upper"].contains(resource) };
