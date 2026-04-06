Follow these steps exactly in order.

## 1. Check for updates

Run:

```
cargo update --dry-run 2>&1
```

If the output contains no lines matching "Updating" or "Adding", the project is already up to date. Print "No dependency updates available." and exit. Do not create a branch, commit, or PR.

## 2. Apply updates

Run:

```
cargo update 2>&1
```

Save the full output — you will include it in the PR body.

## 3. Code quality checks

Run:

```
just full-check
```

If there are formatting issues, fix them with:

```
just full-write
```

Then run `just full-check` again.

If clippy errors remain after formatting, read the failing code, fix the issues, and re-run `just full-check` until it passes.

## 4. Run tests

Run:

```
just test
```

If tests fail, investigate and fix the issues. Re-run `just test` to confirm they pass.

If you cannot fix a test failure, revert all changes (`git checkout .`), print "FAILED: could not resolve test failures after cargo update", and exit.

## 5. Create branch, commit, and PR

Create a branch and commit:

```
git checkout -b chore/cargo-update-$(date +%Y-%m-%d)
git add -A
git commit -m "chore: update cargo dependencies"
```

Push and create a PR:

```
git push -u origin HEAD
```

Create a pull request with:
- Title: `chore: update cargo dependencies`
- Body: a summary section listing the updated crates (from the `cargo update` output in step 2), followed by a note on whether any code changes were needed to pass checks

Print the PR URL.
