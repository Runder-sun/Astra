export type ProviderErrorKind = "configuration" | "capacity";

const PROVIDER_CONFIGURATION_ERROR_PATTERNS = [
	/\b(?:http(?: status)?\s*)?(?:401|403)\b/iu,
	/\binvalid[\s_-]*api[\s_-]*key\b/iu,
	/\b(?:authentication|authorization)\s+(?:failed|failure|required|denied|error)\b/iu,
	/\b(?:missing|no)\s+(?:provider\s+)?(?:api\s+key|credentials?)\b/iu,
	/\b(?:api\s+key|credentials?)\s+(?:are?\s+)?(?:missing|invalid|unavailable|not configured)\b/iu,
	/\b(?:invalid|unknown|unavailable|unsupported)\s+(?:provider\s+)?model\b/iu,
	/\bmodel\b[^\n]{0,160}\b(?:not found|unavailable|invalid|unsupported|not configured|access denied)\b/iu,
	/\b(?:no access|do not have access)\b[^\n]{0,160}\bmodel\b/iu,
];

const PROVIDER_CAPACITY_ERROR_PATTERNS = [
	/\b(?:http(?: status)?\s*)?(?:429|500|502|503|504)\b/iu,
	/\brate[-_\s]?limit(?:ed)?\b/iu,
	/\b(?:servers?\s+are\s+currently\s+)?overloaded(?:[_\s-]error)?\b/iu,
	/\btemporarily[_\s-]unavailable\b/iu,
	/\bservice\s+unavailable\b/iu,
];

export function classifyProviderErrorMessage(message: string): ProviderErrorKind | undefined {
	if (PROVIDER_CONFIGURATION_ERROR_PATTERNS.some((pattern) => pattern.test(message))) return "configuration";
	if (PROVIDER_CAPACITY_ERROR_PATTERNS.some((pattern) => pattern.test(message))) return "capacity";
	return undefined;
}
