import Link from "next/link";

export default function Custom404() {
  return (
    <div
      style={{
        minHeight: "100vh",
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        padding: "3rem 1.5rem",
        fontFamily:
          '-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Oxygen, Ubuntu, sans-serif',
      }}
    >
      <h1
        style={{
          fontSize: "5rem",
          lineHeight: 1,
          fontWeight: 700,
          opacity: 0.5,
          margin: "0 0 0.75rem",
          letterSpacing: "-0.04em",
        }}
      >
        404
      </h1>

      <p
        style={{
          fontSize: "1.125rem",
          opacity: 0.6,
          margin: "0 0 2.5rem",
          maxWidth: "28rem",
          textAlign: "center",
        }}
      >
        This page doesn&apos;t exist. It may have been moved or removed.
      </p>

      <Link
        href="/getting-started"
        style={{
          display: "inline-flex",
          alignItems: "center",
          gap: "0.5rem",
          padding: "0.75rem 1.5rem",
          borderRadius: "0.5rem",
          border: "1px solid rgba(255,255,255,0.1)",
          fontSize: "0.9375rem",
          fontWeight: 600,
          textDecoration: "none",
        }}
      >
        &larr; Back to Docs
      </Link>
    </div>
  );
}
