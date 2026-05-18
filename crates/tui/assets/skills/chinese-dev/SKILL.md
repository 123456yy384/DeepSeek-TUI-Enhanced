---
name: chinese-dev
description: Generate Chinese documentation, bilingual READMEs, WeChat/Feishu format content, and localized notes for Chinese-speaking developers and communities.
---

# Chinese Dev

Use this skill when targeting Chinese developer communities or producing
Chinese-language project artifacts. Covers bilingual README generation,
Chinese technical doc conventions, WeChat official account formatting,
Feishu document export prep, and China-specific deployment notes.

## Workflow

1. Determine the target audience and platform: GitHub (bilingual), WeChat
   Official Account (rich text), Feishu (block format), or generic Chinese docs.
2. **Bilingual README**: Generate `README.md` in English, then a
   `README.zh-CN.md` with natural, native-level Chinese. Prefer concise
   technical tone over literal translation.
3. **WeChat formatting**: Output uses no Markdown tables (WeChat doesn't
   render them). Use emoji-delimited sections, short paragraphs, and numbered
   lists. Keep line width under 20 Chinese characters for mobile readability.
4. **Feishu export**: Use structured blocks, avoid nested tables. Include
   document metadata (author, date, version) at the top.
5. **Chinese technical conventions**:
   - Keep English terms where Chinese translations are not widely adopted
     (e.g., "API", "Token", "Prompt" — don't force-translate).
   - Use Chinese punctuation in Chinese paragraphs (，。；："").
   - Place a space between CJK characters and Latin letters/numbers.
6. **Community notes**: When contributing to Chinese open-source communities
   (Gitee, OpenI, etc.), add the platform-specific badge and mirror links.

## Constraints

- Never use machine-translated placeholder text in final output.
- Verify proper nouns (company names, product names) against official Chinese
  translations before using them.
- For compliance-sensitive content, flag and ask the user to review.
