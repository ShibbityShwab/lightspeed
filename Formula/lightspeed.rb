class Lightspeed < Formula
  desc "Zero-cost global network optimizer for multiplayer games"
  homepage "https://github.com/ShibbityShwab/lightspeed"
  version "1.6.3"
  # Custom noncommercial license (LightSpeed-NC-1.0), not an SPDX identifier.
  license :cannot_represent

  on_macos do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.3/lightspeed-client-x86_64-apple-darwin.tar.xz"
      sha256 "e84ae0ad34e7af190e2c1b65e5d6cdb4521a94febd3cb9c3e4b5676e75d0519b"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.3/lightspeed-client-aarch64-apple-darwin.tar.xz"
      sha256 "4f6f46c35ff668aa1cc325e38e2536f3517a68cc30e83fd21c0f5a9ab9f5dbc2"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.3/lightspeed-client-x86_64-unknown-linux-gnu.tar.xz"
      sha256 "0632794b20c07dae744c052b27ffade3c634721734e46f48f21f10a08930cb33"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.3/lightspeed-client-aarch64-unknown-linux-gnu.tar.xz"
      sha256 "46b040444afb52816fb47725edd6da4ab9d363c628e9194742ee3b0c66be2766"
    end
  end

  def install
    bin.install "lightspeed"
  end

  test do
    assert_match "lightspeed", shell_output("#{bin}/lightspeed --version")
  end
end
