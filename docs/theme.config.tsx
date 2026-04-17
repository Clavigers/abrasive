import React from "react";
import { useConfig, useTheme } from "nextra-theme-docs";
import { useRouter } from "next/router";

const config = {
  logo: (
    <span style={{ fontWeight: 700, fontSize: "1.25rem", letterSpacing: "-0.02em" }}>
      Abrasive
    </span>
  ),
  head: () => {
    const { title } = useConfig();
    const fallbackTitle = "Abrasive Documentation";

    return (
      <>
        <title>{title ? `${title} - ${fallbackTitle}` : fallbackTitle}</title>
        <meta name="viewport" content="width=device-width, initial-scale=1.0" />
        <link rel="icon" type="image/png" href="/favicon.ico" />
      </>
    );
  },
  primaryHue: {
    dark: 210,
    light: 210,
  },
  primarySaturation: {
    dark: 60,
    light: 60,
  },
  logoLink: "/",
  project: {
    link: "https://github.com/Clavigers/abrasive",
  },
  docsRepositoryBase:
    "https://github.com/Clavigers/abrasive/blob/master/docs",
  feedback: {
    labels: "Feedback",
    useLink: () =>
      `https://github.com/Clavigers/abrasive/issues/new`,
  },
  footer: {
    content: "Abrasive - Remote build orchestration",
  },
  toc: {
    backToTop: true,
  },
  sidebar: {
    defaultMenuCollapseLevel: 1,
    toggleButton: false,
    autoCollapse: true,
  },
  darkMode: true,
  nextThemes: {
    defaultTheme: "dark",
  },
  themeSwitch: {
    useOptions() {
      return {
        dark: "Dark",
        light: "Light",
      };
    },
  },
};

export default config;
