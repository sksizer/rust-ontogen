Fetch the latest state of the upstream template repository and compare it against this project.

Apply any new or changed files from the template, respecting project-specific overrides.

After making changes, run code quality checks:

```
just full-check
```

If there are formatting issues, fix them with:

```
just full-write
```

Then run `just full-check` again to confirm everything passes.

When finished, create a branch named `chore/update-from-template` and a single commit with the following format:

```
chore: update project from upstream template

Performed the following:
- <list each change made>

Did not bring over the following because of project-specific overrides:
- <list anything intentionally skipped, or "None">
```

Then push the branch and create a pull request with:
- Title: `chore: update from upstream template`
- Body summarizing what was changed and what was intentionally skipped
