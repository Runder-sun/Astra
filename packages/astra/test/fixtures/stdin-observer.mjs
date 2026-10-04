const original = process.stdin[Symbol.asyncIterator];
process.stdin[Symbol.asyncIterator] = async function* () {
	const iterable = { [Symbol.asyncIterator]: () => original.call(process.stdin) };
	for await (const chunk of iterable) {
		process.send?.({ type: "stdin-chunk" });
		yield chunk;
	}
};
