//! Linux: Landlock, the kernel's own sandbox for unprivileged processes, and a network
//! namespace for a program that may reach only its broker.

pub mod netns;
pub mod sandbox;
pub mod seccomp;
