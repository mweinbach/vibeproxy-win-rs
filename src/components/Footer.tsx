export default function Footer() {
  return (
    <div className="footer">
      <div className="footer-line">
        <span>VibeProxy v0.1.0 was made possible thanks to </span>
        <a
          href="https://github.com/router-for-me/CLIProxyAPIPlus"
          target="_blank"
          rel="noreferrer"
        >
          CLIProxyAPIPlus
        </a>
        <span> | License: MIT</span>
      </div>
      <div className="footer-line">
        <span>&copy; 2026 </span>
        <a href="https://automaze.io" target="_blank" rel="noreferrer">
          Automaze, Ltd.
        </a>
        <span> All rights reserved.</span>
      </div>
      <div className="footer-line footer-report">
        <a
          href="https://github.com/automazeio/vibeproxy/issues"
          target="_blank"
          rel="noreferrer"
        >
          Report an issue
        </a>
      </div>
    </div>
  );
}
