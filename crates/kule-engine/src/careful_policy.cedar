
// Careful: every change to a file or to git waits for a person. Only the
// connections above pass on their own.
@id("careful")
forbid (principal, action == Kule::Action::"auto", resource)
when { resource.kind like "fs.*" || resource.kind like "git.*" };
