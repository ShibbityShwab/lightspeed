class Lightspeed < Formula
  desc "Zero-cost global network optimizer for multiplayer games"
  homepage "https://github.com/ShibbityShwab/lightspeed"
  version "1.11.4"
  # Custom noncommercial license (LightSpeed-NC-1.1), not an SPDX identifier.
  license :cannot_represent

  on_macos do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.11.4/lightspeed-client-x86_64-apple-darwin.tar.xz"
      sha256 "d05d9133982432a4cccac7ee66b0310d6ff06b5efde8fb20ab21ad6871f5e7eb"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.11.4/lightspeed-client-aarch64-apple-darwin.tar.xz"
      sha256 "7e369c72018b4317563920c50fafccef16e8bc18286f3becc3d6737a5990cef6"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.11.4/lightspeed-client-x86_64-unknown-linux-gnu.tar.xz"
      sha256 "7835f62375f2c2532f8c12865d14671c561c50d44a83f9705eb5d4847525f6a0"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.11.4/lightspeed-client-aarch64-unknown-linux-gnu.tar.xz"
      sha256 "7e2e3b3a8acf4d9736d6d1ee186bcc4c6a2af406b81cca35ec58f0098962de90"
    end
  end

  def install
    bin.install "lightspeed"
  end

  test do
    assert_match "lightspeed", shell_output("#{bin}/lightspeed --version")
  end
end
