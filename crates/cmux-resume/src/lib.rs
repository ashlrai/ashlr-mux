//! cmux-resume — SurfaceResume approval subsystem.
//!
//! Headless port of the approval-signing / trust-decision logic from the
//! canonical macOS `Sources/SessionPersistence.swift`: a shell-token lexer, an
//! HMAC-SHA256 approval signature, longest-prefix approval-record matching, and
//! the trust-decision functions that gate whether a restored surface may
//! auto-run its resume command. All I/O is behind injected seams so the whole
//! subsystem is unit-testable with tempfiles and a fixed signing secret.
//!
//! Scaffold — modules are filled in by the port lane.
