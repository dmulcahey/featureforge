const KEY_VALUE_PATTERN = /^([^:]+):\s*(.*)$/;

function normalizeConfigText(configText: string): string[] {
  if (configText.length === 0) {
    return [];
  }

  return configText
    .replace(/\r\n/g, '\n')
    .split('\n')
    .filter((line, index, lines) => !(index === lines.length - 1 && line === ''));
}

export function getConfigValue(configText: string, key: string): string {
  let matchedValue = '';

  for (const line of normalizeConfigText(configText)) {
    const match = line.match(KEY_VALUE_PATTERN);
    if (match && match[1] === key) {
      matchedValue = match[2].replace(/\s+/g, '');
    }
  }

  return matchedValue;
}

export function setConfigValue(configText: string, key: string, value: string): string {
  const normalizedLines = normalizeConfigText(configText);
  const updatedLines: string[] = [];
  let updated = false;

  for (const line of normalizedLines) {
    const match = line.match(KEY_VALUE_PATTERN);
    if (match && match[1] === key) {
      updatedLines.push(`${key}: ${value}`);
      updated = true;
      continue;
    }

    updatedLines.push(line);
  }

  if (!updated) {
    updatedLines.push(`${key}: ${value}`);
  }

  return updatedLines.length > 0 ? `${updatedLines.join('\n')}\n` : '';
}
