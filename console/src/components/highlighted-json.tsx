"use client";

import { useState, useEffect } from "react";
import { codeToHtml } from "shiki";

export function HighlightedJson({ code }: { code: string }) {
  const [html, setHtml] = useState<string>("");

  useEffect(() => {
    let cancelled = false;
    codeToHtml(code, {
      lang: "json",
      themes: { light: "github-light", dark: "github-dark" },
      defaultColor: false,
    }).then((result) => {
      if (!cancelled) setHtml(result);
    });
    return () => {
      cancelled = true;
    };
  }, [code]);

  if (!html) {
    return (
      <pre className="whitespace-pre-wrap font-mono text-xs p-4">{code}</pre>
    );
  }

  return (
    <div
      className="p-4 text-xs [&_pre]:!bg-transparent [&_code]:!bg-transparent [&_.shiki]:!bg-transparent"
      dangerouslySetInnerHTML={{ __html: html }}
    />
  );
}
