class Lightspeed < Formula
  desc "Zero-cost global network optimizer for multiplayer games"
  homepage "https://github.com/ShibbityShwab/lightspeed"
  version "1.6.8"
  # Custom noncommercial license (LightSpeed-NC-1.0), not an SPDX identifier.
  license :cannot_represent

  on_macos do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.8/lightspeed-client-x86_64-apple-darwin.tar.xz"
      sha256 "68c74ed49ba72d444f6edb267c33e0cc7a772fa47566592af27068902511989f"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.8/lightspeed-client-aarch64-apple-darwin.tar.xz"
      sha256 "2fcd38f30b0d3ebdb59e521f63c6088596ab8dcf60b43c4fb391586e1466a749"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.8/lightspeed-client-x86_64-unknown-linux-gnu.tar.xz"
      sha256 "17508058bb3a86736f2688a95aa60f79d3376c1e341e1a645fd078474bc3396b"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.8/lightspeed-client-aarch64-unknown-linux-gnu.tar.xz"
      sha256 "20a04718a2a4ae05f583fc14c9ed15d0b26a1d0d8166e84ba8a89a1d4121ab7d"
    end
  end

  def install
    bin.install "lightspeed"
  end

  test do
    assert_match "lightspeed", shell_output("#{bin}/lightspeed --version")
  end
end
