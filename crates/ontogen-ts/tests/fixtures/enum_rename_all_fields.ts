export type Event = { toolCall: { promptTemplate: string } } | { toolResult: { exitCode: number } };
