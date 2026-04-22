export default function Footer() {
  return (
    <footer className="site-footer">
      <div className="footer-inner">
        <div className="footer-col">
          <h4>Abrasive</h4>
          <a href="/">Home</a>
          <a href="/demo">Demo</a>
          <a href="/getting-started">Get Started</a>
        </div>
        <div className="footer-col">
          <h4>Install</h4>
          <a href="/get_abrasive">Get Abrasive</a>
          <a href="/getting-started#alias">Cargo Alias</a>
        </div>
        <div className="footer-col">
          <h4>Community</h4>
          <a href="https://github.com/Clavigers/abrasive-cli">GitHub</a>
          <a href="https://github.com/Clavigers/abrasive-cli/issues">Issues</a>
          <a href="https://github.com/Clavigers/abrasive-cli/discussions">Discussions</a>
        </div>
      </div>
      <div className="footer-bottom">
        <span>&copy; 2026 Claviger.</span>
      </div>
    </footer>
  )
}
