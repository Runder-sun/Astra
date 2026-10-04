export function textTail(text: string, limit: number): string {
	const tail = text.slice(-limit);
	const first = tail.charCodeAt(0);
	return first >= 0xdc00 && first <= 0xdfff ? tail.slice(1) : tail;
}
