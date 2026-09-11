const targetBytes = 250 * 1024;

function section(index: number): string {
  return [
    `## Section ${index}`,
    "",
    `Paragraph ${index} contains **portable Markdown**, _emphasis_, a [reference link](https://example.com/reference), and representative prose long enough to exercise parsing, layout, wrapping, and painting in the visual editor without relying on generated filler tokens.`,
    "",
    `> Review note ${index}: keep this document readable, portable, and safe to share with colleagues using other Markdown applications.`,
    "",
    `- Item ${index}.1 preserves ordinary list behavior.`,
    `- Item ${index}.2 includes \`inline code\` and ~~completed wording~~.`,
    `- [ ] Review section ${index} before circulation.`,
    "",
    "| Topic | Status |",
    "| --- | --- |",
    `| Compatibility ${index} | Ready for review |`,
    "",
    "```text",
    `representative fenced block ${index}`,
    "```",
  ].join("\n");
}

export function normalPerformanceDocument(): string {
  const encoder = new TextEncoder();
  const sections: string[] = [];
  let byteLength = 0;
  for (let index = 1; byteLength < targetBytes; index += 1) {
    const next = section(index);
    sections.push(next);
    byteLength += encoder.encode(next).byteLength + (sections.length === 1 ? 0 : 2);
  }
  return sections.join("\n\n");
}
