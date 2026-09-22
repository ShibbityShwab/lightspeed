class Lightspeed < Formula
  desc "Zero-cost global network optimizer for multiplayer games"
  homepage "https://github.com/ShibbityShwab/lightspeed"
  version "1.6.4"
  # Custom noncommercial license (LightSpeed-NC-1.0), not an SPDX identifier.
  license :cannot_represent

  on_macos do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.4/lightspeed-client-x86_64-apple-darwin.tar.xz"
      sha256 "8a3ed7f663738c4a4b4a11267d01fcf6f1aaf9a49f160aad197a9411da76356c"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.4/lightspeed-client-aarch64-apple-darwin.tar.xz"
      sha256 "c330fcea14fa58088465cb7c3bc0f0571bf972ef2c7d057c520b3b822b3783f3"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.4/lightspeed-client-x86_64-unknown-linux-gnu.tar.xz"
      sha256 "162999b4910d9f47801212d1a03196ccbb7ecb5c7822df7ea4bf9c2838890fe1"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.4/lightspeed-client-aarch64-unknown-linux-gnu.tar.xz"
      sha256 "d1a9c4771118834c601bf96101ca55be982806db7709ff49687c3632a7f04859"
    end
  end

  def install
    bin.install "lightspeed"
  end

  test do
    assert_match "lightspeed", shell_output("#{bin}/lightspeed --version")
  end
end
