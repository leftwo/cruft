# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## General Development Guidelines

### Git Workflow

**IMPORTANT:** When committing changes:
- NEVER use `git add -A` or `git add .`
- ALWAYS specify individual files explicitly: `git add file1 file2 file3`
- This prevents accidentally committing untracked files or temporary files
- After making changes, propose files to add and commit message, wait for user approval
- Do NOT include emoji or tool references (like "🤖 Generated with Claude Code") in commit messages
- Keep git commit lines to 80 characters wide, but take as many lines as needed

### Rust Development

**Standard Workflow:**
```bash
# After making changes, always run:
cargo fmt && cargo clippy --all-targets && cargo test
```

**Code Quality:**
- Use Rust edition 2024
- Honor rustfmt settings (max_width = 80)
- ALWAYS fix clippy warnings - do not ignore them unless specifically instructed
- Do NOT add `-D warnings` flag to clippy commands
- Use `cargo check` for quick validation instead of `cargo build --release`

**Project Structure:**
- Keep crates at top level, not in subdirectories
- Use the rust clap library for command line processing

## Repository Overview

This repository contains multiple independent programs. Each program lives in its own subdirectory.

## Project: CRS (Central Registry Service)

**Location:** `crs/` directory

The CRS is a client/server system where clients register with a central service to advertise their presence and status. The server provides a web interface to view all connected clients in real-time.

## Project: OxMon (Network Monitoring)

**Location:** `oxmon/` directory

OxMon is a network monitoring tool that pings hosts and tracks their status over time. It provides a web dashboard and CLI for viewing host status.
