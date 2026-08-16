export type Event = { toolCall: { PROMPT_TEMPLATE: string } } | { toolResult: { exitCode: number } };
