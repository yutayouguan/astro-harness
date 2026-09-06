export function formatDiagnosticContext(
  sessionId: string,
  turnId: string,
): string {
  return `session_id=${sessionId}\nturn_id=${turnId}\n`;
}
