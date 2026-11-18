class Lightspeed < Formula
  desc "Zero-cost global network optimizer for multiplayer games"
  homepage "https://github.com/ShibbityShwab/lightspeed"
  version "1.11.2"
  # Custom noncommercial license (LightSpeed-NC-1.1), not an SPDX identifier.
  license :cannot_represent

  on_macos do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.11.2/lightspeed-client-x86_64-apple-darwin.tar.xz"
      sha256 "74c1929d33a28f1926455d1f677f9f93173cd285486c2051c8eae15d926090d8"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.11.2/lightspeed-client-aarch64-apple-darwin.tar.xz"
      sha256 "8d21208f2c10dbc142c5c5b1d42e1728a3ac031915e45b4e1ddbe54db8566a8d"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.11.2/lightspeed-client-x86_64-unknown-linux-gnu.tar.xz"
      sha256 "6b7f6ae2a650c681c57f97d991de362134a20c6f504d0d282229e03826d6d167"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.11.2/lightspeed-client-aarch64-unknown-linux-gnu.tar.xz"
      sha256 "0778a69c9b4bf7aff0ac2b4df600210f8932526b47ef4172f752d269f79398ac"
    end
  end

  def install
    bin.install "lightspeed"
  end

  test do
    assert_match "lightspeed", shell_output("#{bin}/lightspeed --version")
  end
end
