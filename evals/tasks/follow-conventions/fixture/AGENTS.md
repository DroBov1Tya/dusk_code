# Rules for this project

- Every function that changes data records it with `audit(action, target)`.
- Errors and events are never printed.
- Each public function has a docstring of one line.
